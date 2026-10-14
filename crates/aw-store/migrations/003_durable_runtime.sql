-- Durable first-class session/runtime recovery. This migration is additive.
-- It does not enable direct multi-process ownership; owner.lock remains authoritative.
CREATE TABLE store_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO store_meta(key,value) VALUES('store_id','store_' || lower(hex(randomblob(16))));
INSERT INTO store_meta(key,value) VALUES('schema_epoch','3');
INSERT INTO store_meta(key,value) VALUES('restore_generation','1');

CREATE TABLE commit_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    store_id TEXT NOT NULL,
    tx_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(length(kind) BETWEEN 1 AND 64),
    entity_type TEXT NOT NULL CHECK(length(entity_type) BETWEEN 1 AND 64),
    entity_id TEXT NOT NULL CHECK(length(entity_id) BETWEEN 1 AND 160),
    actor TEXT,
    schema_version INTEGER NOT NULL CHECK(schema_version>0),
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json) AND length(payload_json)<=16384),
    created_at INTEGER NOT NULL
);
CREATE INDEX commit_log_by_entity ON commit_log(entity_type,entity_id,seq);
CREATE TRIGGER commit_log_immutable_update BEFORE UPDATE ON commit_log BEGIN SELECT RAISE(ABORT,'commit log immutable'); END;
CREATE TRIGGER commit_log_immutable_delete BEFORE DELETE ON commit_log BEGIN SELECT RAISE(ABORT,'commit log immutable'); END;

CREATE TABLE branch_ref_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    branch_id TEXT NOT NULL REFERENCES branches(id),
    old_head TEXT,
    old_revision INTEGER,
    new_head TEXT NOT NULL,
    new_revision INTEGER NOT NULL CHECK(new_revision>=0),
    actor TEXT NOT NULL,
    client_id TEXT,
    cause_type TEXT NOT NULL CHECK(length(cause_type) BETWEEN 1 AND 64),
    cause_id TEXT,
    committed_at INTEGER NOT NULL
);
CREATE INDEX branch_ref_events_by_branch ON branch_ref_events(branch_id,seq);
CREATE TRIGGER branch_ref_events_immutable_update BEFORE UPDATE ON branch_ref_events BEGIN SELECT RAISE(ABORT,'branch ref events immutable'); END;
CREATE TRIGGER branch_ref_events_immutable_delete BEFORE DELETE ON branch_ref_events BEGIN SELECT RAISE(ABORT,'branch ref events immutable'); END;

-- One logical user/autonomous operation may have several real executions after
-- explicit/policy retry. Attempts are never overwritten or reinterpreted.
CREATE TABLE llm_operations (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    origin_branch_id TEXT NOT NULL REFERENCES branches(id),
    origin_head TEXT NOT NULL REFERENCES nodes(id),
    origin_revision INTEGER NOT NULL CHECK(origin_revision>=0),
    prompt_node TEXT REFERENCES nodes(id),
    actor TEXT NOT NULL,
    identity_json TEXT NOT NULL CHECK(json_valid(identity_json)),
    retry_policy_json TEXT NOT NULL CHECK(json_valid(retry_policy_json)),
    kind TEXT NOT NULL CHECK(kind IN ('interactive','autonomous')),
    state TEXT NOT NULL CHECK(state IN ('running','completed','failed','interrupted','needs_attention','retry_scheduled','exhausted')),
    created_at INTEGER NOT NULL,
    settled_at INTEGER
);
CREATE INDEX llm_operations_by_session ON llm_operations(session_id,created_at,id);

ALTER TABLE runs ADD COLUMN operation_id TEXT REFERENCES llm_operations(id);
ALTER TABLE runs ADD COLUMN attempt_no INTEGER NOT NULL DEFAULT 1 CHECK(attempt_no>0);
ALTER TABLE runs ADD COLUMN produced_from_head TEXT;
ALTER TABLE runs ADD COLUMN produced_from_revision INTEGER;
ALTER TABLE runs ADD COLUMN attach_status TEXT NOT NULL DEFAULT 'pending' CHECK(attach_status IN ('pending','attached','conflict','unattached'));

-- Legacy rows become one-attempt operations without inventing retry authority.
INSERT INTO llm_operations(id,session_id,origin_branch_id,origin_head,origin_revision,prompt_node,actor,identity_json,retry_policy_json,kind,state,created_at,settled_at)
SELECT 'op_' || id, session_id, branch_id,
       (SELECT parent_id FROM nodes WHERE id=prompt_node),
       CASE WHEN (SELECT revision FROM branches WHERE id=branch_id)>0 THEN (SELECT revision FROM branches WHERE id=branch_id)-1 ELSE 0 END,
       prompt_node,
       json_extract(identity_json,'$.actor'), identity_json,
       '{"mode":"interactive","max_attempts":16,"retry_unknown":false,"initial_backoff_ms":1000,"max_backoff_ms":60000}',
       'interactive',
       CASE status WHEN 'running' THEN 'interrupted' WHEN 'completed' THEN 'completed' ELSE 'failed' END,
       started_at,ended_at
FROM runs;
UPDATE runs SET operation_id='op_' || id,
    produced_from_head=(SELECT parent_id FROM nodes WHERE id=prompt_node),
    produced_from_revision=CASE WHEN (SELECT revision FROM branches WHERE id=branch_id)>0 THEN (SELECT revision FROM branches WHERE id=branch_id)-1 ELSE 0 END,
    attach_status=CASE WHEN final_node IS NOT NULL THEN 'attached' ELSE 'unattached' END
WHERE operation_id IS NULL;

CREATE TABLE llm_attempts (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL REFERENCES llm_operations(id),
    attempt_no INTEGER NOT NULL CHECK(attempt_no>0),
    run_id TEXT NOT NULL UNIQUE REFERENCES runs(id),
    dispatch_state TEXT NOT NULL CHECK(dispatch_state IN ('admitted','provider_started','provider_settled','unknown')),
    outcome TEXT,
    known_usage_json TEXT CHECK(known_usage_json IS NULL OR json_valid(known_usage_json)),
    started_at INTEGER NOT NULL,
    settled_at INTEGER,
    UNIQUE(operation_id,attempt_no)
);
CREATE INDEX llm_attempts_by_operation ON llm_attempts(operation_id,attempt_no);

INSERT INTO llm_attempts(id,operation_id,attempt_no,run_id,dispatch_state,outcome,started_at,settled_at)
SELECT 'att_' || id,operation_id,attempt_no,id,
       CASE WHEN EXISTS(SELECT 1 FROM invocation_receipts i WHERE i.run_id=runs.id AND i.state='started') THEN 'unknown'
            WHEN EXISTS(SELECT 1 FROM invocation_receipts i WHERE i.run_id=runs.id) THEN 'provider_settled'
            ELSE 'admitted' END,
       status,started_at,ended_at
FROM runs;

CREATE TABLE retry_schedules (
    operation_id TEXT PRIMARY KEY REFERENCES llm_operations(id),
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation>0),
    state TEXT NOT NULL CHECK(state IN ('waiting_user','scheduled','claimed','cancelled','completed','exhausted','blocked')),
    due_at INTEGER,
    next_attempt_no INTEGER NOT NULL CHECK(next_attempt_no>1),
    claimed_by_client TEXT,
    claim_generation INTEGER,
    updated_at INTEGER NOT NULL
);

CREATE TABLE recovery_hints (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL REFERENCES llm_operations(id),
    session_id TEXT NOT NULL REFERENCES sessions(id),
    branch_id TEXT NOT NULL REFERENCES branches(id),
    kind TEXT NOT NULL CHECK(kind IN ('llm_unknown','retry_available','retry_scheduled','effects_unknown','attachment_conflict')),
    severity TEXT NOT NULL CHECK(severity IN ('info','warning','blocking')),
    message TEXT NOT NULL CHECK(length(message) BETWEEN 1 AND 2048),
    action_json TEXT NOT NULL CHECK(json_valid(action_json)),
    state TEXT NOT NULL CHECK(state IN ('open','resolved','dismissed')),
    created_at INTEGER NOT NULL,
    resolved_at INTEGER
);
CREATE INDEX recovery_hints_by_session ON recovery_hints(session_id,state,created_at,id);


-- Logical policy/lease/migration planes. They remain in the same SQLite file;
-- direct multi-process writers are not enabled until these are exercised by the
-- runtime and stress-tested.
CREATE TABLE branch_policies (
    branch_id TEXT PRIMARY KEY REFERENCES branches(id),
    policy_kind TEXT NOT NULL CHECK(policy_kind IN ('private','collaborative','released')),
    owner_principal TEXT NOT NULL,
    policy_json TEXT NOT NULL CHECK(json_valid(policy_json)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    updated_at INTEGER NOT NULL
);
CREATE TABLE policy_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    actor TEXT NOT NULL,
    old_revision INTEGER,
    new_revision INTEGER NOT NULL,
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
    created_at INTEGER NOT NULL
);
CREATE TRIGGER policy_events_immutable_update BEFORE UPDATE ON policy_events BEGIN SELECT RAISE(ABORT,'policy events immutable'); END;
CREATE TRIGGER policy_events_immutable_delete BEFORE DELETE ON policy_events BEGIN SELECT RAISE(ABORT,'policy events immutable'); END;

CREATE TABLE client_runtimes (
    client_id TEXT PRIMARY KEY,
    principal TEXT NOT NULL,
    process_kind TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>0),
    started_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    stopped_at INTEGER
);
CREATE TABLE execution_leases (
    resource_key TEXT PRIMARY KEY,
    holder_client_id TEXT NOT NULL REFERENCES client_runtimes(client_id),
    run_id TEXT REFERENCES runs(id),
    generation INTEGER NOT NULL CHECK(generation>0),
    mode TEXT NOT NULL CHECK(mode IN ('read','write')),
    state TEXT NOT NULL CHECK(state IN ('active','expired','hazard','released')),
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE schema_meta (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    schema_epoch INTEGER NOT NULL,
    activation_epoch INTEGER NOT NULL,
    migration_phase TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
INSERT INTO schema_meta VALUES(1,3,3,'ready',0);
CREATE TABLE migration_lease (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    holder_client_id TEXT,
    generation INTEGER NOT NULL DEFAULT 0,
    current_epoch INTEGER NOT NULL,
    target_epoch INTEGER,
    lease_expires_at INTEGER,
    updated_at INTEGER NOT NULL
);
INSERT INTO migration_lease(singleton,generation,current_epoch,updated_at) VALUES(1,0,3,0);
CREATE TABLE migration_state (
    migration_id TEXT PRIMARY KEY,
    from_epoch INTEGER NOT NULL,
    to_epoch INTEGER NOT NULL,
    stage TEXT NOT NULL CHECK(stage IN ('pending','schema_expanded','backfilling','backfill_complete','compatibility_verified','activated','contract_pending','complete','failed')),
    cursor TEXT,
    holder_generation INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    last_progress_at INTEGER NOT NULL,
    activated_at INTEGER,
    error TEXT
);

-- Stable users/auth metadata. Secrets/bearer values are not stored here.
CREATE TABLE users (
    user_id TEXT PRIMARY KEY,
    realm_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','suspended','deleted')),
    display_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(display_json)),
    security_epoch INTEGER NOT NULL DEFAULT 1 CHECK(security_epoch>0),
    created_at INTEGER NOT NULL
);
CREATE TABLE user_identities (
    identity_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    kind TEXT NOT NULL CHECK(kind IN ('legacy_principal','local','oidc')),
    provider_id TEXT,
    issuer TEXT,
    subject TEXT,
    normalized_login TEXT,
    external_key TEXT NOT NULL UNIQUE,
    verified_at INTEGER,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','revoked'))
);
CREATE UNIQUE INDEX oidc_identity_unique ON user_identities(provider_id,issuer,subject) WHERE kind='oidc';
CREATE TEMP TABLE legacy_principals(principal TEXT PRIMARY KEY,user_id TEXT NOT NULL);
INSERT INTO legacy_principals(principal,user_id)
SELECT principal,'usr_' || lower(hex(randomblob(16))) FROM (
    SELECT owner AS principal FROM sessions UNION SELECT principal FROM members
);
INSERT INTO users(user_id,realm_id,status,display_json,created_at)
SELECT user_id,'legacy','active',json_object('legacy_principal',principal),0 FROM legacy_principals;
INSERT INTO user_identities(identity_id,user_id,kind,external_key,verified_at)
SELECT 'ident_' || lower(hex(randomblob(16))),user_id,'legacy_principal',principal,0 FROM legacy_principals;
ALTER TABLE sessions ADD COLUMN owner_user_id TEXT REFERENCES users(user_id);
UPDATE sessions SET owner_user_id=(SELECT user_id FROM legacy_principals WHERE principal=sessions.owner);
ALTER TABLE members ADD COLUMN principal_user_id TEXT REFERENCES users(user_id);
UPDATE members SET principal_user_id=(SELECT user_id FROM legacy_principals WHERE principal=members.principal);
DROP TABLE legacy_principals;

CREATE TABLE organizations (
    organization_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    security_epoch INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL
);
CREATE TABLE workspace_memberships (
    organization_id TEXT NOT NULL REFERENCES organizations(organization_id),
    user_id TEXT NOT NULL REFERENCES users(user_id),
    role_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    policy_revision INTEGER NOT NULL DEFAULT 1,
    joined_at INTEGER NOT NULL,
    revoked_at INTEGER,
    PRIMARY KEY(organization_id,user_id)
);
CREATE TABLE roles (
    role_id TEXT PRIMARY KEY,
    organization_id TEXT REFERENCES organizations(organization_id),
    name TEXT NOT NULL,
    policy_json TEXT NOT NULL CHECK(json_valid(policy_json)),
    revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE local_auth_credentials (
    credential_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    kind TEXT NOT NULL CHECK(kind IN ('passkey','device_key','password_verifier')),
    public_key TEXT,
    password_hash TEXT,
    credential_counter INTEGER,
    version INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER,
    CHECK(password_hash IS NULL OR kind='password_verifier'),
    CHECK(public_key IS NULL OR kind IN ('passkey','device_key'))
);
CREATE TABLE auth_sessions (
    session_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    credential_id TEXT REFERENCES local_auth_credentials(credential_id),
    auth_method TEXT NOT NULL,
    refresh_token_hash TEXT NOT NULL,
    rotation_family_id TEXT NOT NULL,
    issued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    revoked_at INTEGER,
    security_epoch_at_issue INTEGER NOT NULL,
    client_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(client_json))
);

CREATE TABLE user_devices (
    device_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    device_kind TEXT NOT NULL CHECK(device_kind IN ('native','browser','mobile')),
    public_key TEXT,
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    capabilities_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(capabilities_json)),
    security_epoch INTEGER NOT NULL DEFAULT 1,
    registered_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    revoked_at INTEGER
);
CREATE TABLE provider_accounts (
    provider_account_id TEXT PRIMARY KEY,
    owner_user_id TEXT REFERENCES users(user_id),
    organization_id TEXT REFERENCES organizations(organization_id),
    provider_kind TEXT NOT NULL,
    account_owner TEXT NOT NULL,
    payer TEXT NOT NULL,
    credential_custody TEXT NOT NULL CHECK(credential_custody IN ('server_secret','user_device','browser_device')),
    executor_id TEXT NOT NULL,
    allowed_models_json TEXT NOT NULL CHECK(json_valid(allowed_models_json)),
    policy_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(policy_json)),
    status TEXT NOT NULL CHECK(status IN ('active','revoked','unavailable')),
    secret_ref TEXT,
    secret_version INTEGER,
    created_at INTEGER NOT NULL,
    rotated_at INTEGER,
    revoked_at INTEGER,
    CHECK((credential_custody='server_secret' AND secret_ref IS NOT NULL) OR (credential_custody<>'server_secret' AND secret_ref IS NULL))
);

CREATE TABLE security_audit_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    audit_id TEXT NOT NULL UNIQUE,
    store_id TEXT NOT NULL,
    realm_id TEXT,
    actor_user_id TEXT REFERENCES users(user_id),
    auth_session_id TEXT,
    request_id TEXT,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    outcome TEXT NOT NULL CHECK(outcome IN ('allowed','denied','failed')),
    policy_revision INTEGER,
    credential_handle_id TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(metadata_json) AND length(metadata_json)<=16384),
    created_at INTEGER NOT NULL
);
CREATE TRIGGER security_audit_immutable_update BEFORE UPDATE ON security_audit_events BEGIN SELECT RAISE(ABORT,'security audit immutable'); END;
CREATE TRIGGER security_audit_immutable_delete BEFORE DELETE ON security_audit_events BEGIN SELECT RAISE(ABORT,'security audit immutable'); END;

-- Semantic sync metadata. Raw credentials/auth sessions are not generic sync payloads.
CREATE TABLE sync_peers (
    peer_id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL,
    public_key TEXT,
    trust_scope_json TEXT NOT NULL CHECK(json_valid(trust_scope_json)),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    security_epoch INTEGER NOT NULL DEFAULT 1,
    last_seen_at INTEGER
);
CREATE TABLE sync_cursors (
    peer_id TEXT NOT NULL REFERENCES sync_peers(peer_id),
    source_store_id TEXT NOT NULL,
    last_applied_seq INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(peer_id,source_store_id)
);
CREATE TABLE applied_remote_events (
    event_id TEXT PRIMARY KEY,
    source_store_id TEXT NOT NULL,
    source_seq INTEGER NOT NULL,
    applied_at INTEGER NOT NULL,
    UNIQUE(source_store_id,source_seq)
);
CREATE TABLE sync_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    source_store_id TEXT NOT NULL,
    through_seq INTEGER NOT NULL,
    body_json TEXT NOT NULL CHECK(json_valid(body_json)),
    created_at INTEGER NOT NULL
);
CREATE TRIGGER sync_snapshots_immutable_update BEFORE UPDATE ON sync_snapshots BEGIN SELECT RAISE(ABORT,'sync snapshots immutable'); END;
CREATE TRIGGER sync_snapshots_immutable_delete BEFORE DELETE ON sync_snapshots BEGIN SELECT RAISE(ABORT,'sync snapshots immutable'); END;

PRAGMA user_version=3;
