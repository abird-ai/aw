-- Phase 50: governed guardian review gates and explicit human approval/release receipts.

ALTER TABLE delegation_roots
ADD COLUMN guardian_routing_revision_id TEXT REFERENCES routing_strategy_revisions(revision_id) ON DELETE RESTRICT;

CREATE INDEX delegation_roots_guardian_route
    ON delegation_roots(guardian_routing_revision_id)
    WHERE guardian_routing_revision_id IS NOT NULL;

CREATE TABLE review_operations(
    id TEXT PRIMARY KEY,
    root_id TEXT NOT NULL REFERENCES delegation_roots(id) ON DELETE CASCADE,
    actor_user_id TEXT NOT NULL REFERENCES users(user_id),
    parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    parent_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    parent_turn_id INTEGER NOT NULL CHECK(parent_turn_id>=1),
    source_kind TEXT NOT NULL CHECK(source_kind IN ('team','planner')),
    source_id TEXT NOT NULL,
    source_sha256 TEXT NOT NULL CHECK(length(source_sha256)=64),
    strategy_revision_id TEXT NOT NULL REFERENCES routing_strategy_revisions(revision_id) ON DELETE RESTRICT,
    parent_identity_json TEXT NOT NULL CHECK(json_valid(parent_identity_json) AND length(parent_identity_json)<=16384),
    guardian_identity_json TEXT NOT NULL CHECK(json_valid(guardian_identity_json) AND length(guardian_identity_json)<=16384),
    input_sha256 TEXT NOT NULL CHECK(length(input_sha256)=64),
    policy_sha256 TEXT NOT NULL CHECK(length(policy_sha256)=64),
    policy_text TEXT NOT NULL CHECK(length(policy_text)>=1 AND length(policy_text)<=8192),
    objective_sha256 TEXT NOT NULL CHECK(length(objective_sha256)=64),
    state TEXT NOT NULL CHECK(state IN (
      'admitted','started','pass','deny','waiting_human','needs_evidence',
      'failed','cancelled','unknown','interrupted'
    )),
    verdict_sha256 TEXT CHECK(verdict_sha256 IS NULL OR length(verdict_sha256)=64),
    verdict_json TEXT CHECK(verdict_json IS NULL OR (json_valid(verdict_json) AND length(verdict_json)<=8192)),
    metadata_json TEXT CHECK(metadata_json IS NULL OR (json_valid(metadata_json) AND length(metadata_json)<=16384)),
    error TEXT CHECK(error IS NULL OR length(error)<=4096),
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    settled_at INTEGER,
    UNIQUE(source_kind,source_id),
    CHECK(
      (state='admitted' AND started_at IS NULL AND settled_at IS NULL AND verdict_json IS NULL) OR
      (state='started' AND started_at IS NOT NULL AND settled_at IS NULL AND verdict_json IS NULL) OR
      (state IN ('pass','deny','waiting_human','needs_evidence')
        AND started_at IS NOT NULL AND settled_at IS NOT NULL AND verdict_json IS NOT NULL) OR
      (state IN ('failed','cancelled','unknown','interrupted') AND settled_at IS NOT NULL)
    )
);
CREATE INDEX review_operations_parent
    ON review_operations(parent_session_id,parent_branch_id,parent_turn_id,created_at);
CREATE INDEX review_operations_state ON review_operations(state,created_at);

CREATE TABLE review_human_approvals(
    review_operation_id TEXT PRIMARY KEY REFERENCES review_operations(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK(state IN ('waiting','approved','denied')),
    requested_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    decided_by_user_id TEXT REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    decided_at INTEGER,
    CHECK(
      (state='waiting' AND decided_by_user_id IS NULL AND decided_at IS NULL) OR
      (state IN ('approved','denied') AND decided_by_user_id IS NOT NULL AND decided_at IS NOT NULL)
    )
);
CREATE INDEX review_human_approvals_state ON review_human_approvals(state,created_at);

CREATE TABLE review_release_receipts(
    review_operation_id TEXT PRIMARY KEY REFERENCES review_operations(id) ON DELETE CASCADE,
    source_sha256 TEXT NOT NULL CHECK(length(source_sha256)=64),
    basis TEXT NOT NULL CHECK(basis IN ('guardian_pass','human_approved')),
    released_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL
);

CREATE TRIGGER review_operations_authority_immutable
BEFORE UPDATE OF root_id,actor_user_id,parent_session_id,parent_branch_id,parent_turn_id,
                 source_kind,source_id,source_sha256,strategy_revision_id,parent_identity_json,
                 guardian_identity_json,input_sha256,policy_sha256,policy_text,objective_sha256,created_at
ON review_operations
BEGIN SELECT RAISE(ABORT,'review operation authority snapshot is immutable'); END;

CREATE TRIGGER delegation_root_guardian_route_pin_once
BEFORE UPDATE OF guardian_routing_revision_id ON delegation_roots
WHEN OLD.guardian_routing_revision_id IS NOT NEW.guardian_routing_revision_id
     AND OLD.guardian_routing_revision_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT,'delegation guardian routing revision is immutable once pinned');
END;

CREATE TRIGGER review_human_approval_terminal_update
BEFORE UPDATE OF state ON review_human_approvals
WHEN OLD.state <> 'waiting'
BEGIN SELECT RAISE(ABORT,'review human approval is already terminal'); END;
CREATE TRIGGER review_human_approvals_immutable_delete
BEFORE DELETE ON review_human_approvals
BEGIN SELECT RAISE(ABORT,'review human approvals are immutable'); END;

CREATE TRIGGER review_release_receipts_immutable_update
BEFORE UPDATE ON review_release_receipts
BEGIN SELECT RAISE(ABORT,'review release receipts are immutable'); END;
CREATE TRIGGER review_release_receipts_immutable_delete
BEFORE DELETE ON review_release_receipts
BEGIN SELECT RAISE(ABORT,'review release receipts are immutable'); END;

UPDATE store_meta SET value='32' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=32,activation_epoch=32,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=32,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=32;
