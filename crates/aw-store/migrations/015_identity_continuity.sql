-- Phase 30: cross-host stable aw user identity/auth continuity.
-- Generic Workspace/semantic replication still excludes passwords, bearer/
-- refresh tokens, provider secrets and local Unix/internal identity bindings.

CREATE TABLE identity_continuity_grants (
    grant_id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    source_node_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    accepted_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    accepted_at INTEGER NOT NULL,
    revoked_at INTEGER,
    UNIQUE(domain_id,source_node_id)
);
CREATE INDEX identity_continuity_grants_active
    ON identity_continuity_grants(domain_id,status,source_node_id);

CREATE TABLE identity_continuity_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    source_store_id TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    domain_security_epoch INTEGER NOT NULL CHECK(domain_security_epoch>0),
    snapshot_sha256 TEXT NOT NULL CHECK(length(snapshot_sha256)=64),
    body_json TEXT NOT NULL CHECK(json_valid(body_json)),
    source_signature TEXT NOT NULL,
    imported_by_grant_id TEXT NOT NULL REFERENCES identity_continuity_grants(grant_id),
    imported_at INTEGER NOT NULL,
    UNIQUE(domain_id,source_store_id,snapshot_sha256)
);
CREATE INDEX identity_continuity_snapshots_domain
    ON identity_continuity_snapshots(domain_id,imported_at,snapshot_id);
CREATE TRIGGER identity_continuity_snapshots_immutable_update
BEFORE UPDATE ON identity_continuity_snapshots
BEGIN SELECT RAISE(ABORT,'identity continuity snapshot immutable'); END;
CREATE TRIGGER identity_continuity_snapshots_immutable_delete
BEFORE DELETE ON identity_continuity_snapshots
BEGIN SELECT RAISE(ABORT,'identity continuity snapshot immutable'); END;

CREATE TABLE portable_identity_bindings (
    binding_id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    user_id TEXT NOT NULL REFERENCES users(user_id),
    source_store_id TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('device_key','oidc')),
    binding_key TEXT NOT NULL,
    device_id TEXT,
    public_key TEXT,
    provider_id TEXT,
    issuer TEXT,
    subject TEXT,
    user_security_epoch INTEGER NOT NULL CHECK(user_security_epoch>0),
    domain_security_epoch INTEGER NOT NULL CHECK(domain_security_epoch>0),
    verified_at INTEGER,
    binding_sha256 TEXT NOT NULL CHECK(length(binding_sha256)=64),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    imported_at INTEGER NOT NULL,
    UNIQUE(domain_id,kind,binding_key),
    CHECK((kind='device_key' AND device_id IS NOT NULL AND public_key IS NOT NULL AND issuer IS NULL AND subject IS NULL)
       OR (kind='oidc' AND issuer IS NOT NULL AND subject IS NOT NULL AND public_key IS NULL))
);
CREATE INDEX portable_identity_bindings_user
    ON portable_identity_bindings(domain_id,user_id,status,kind,binding_id);
CREATE INDEX portable_identity_bindings_source
    ON portable_identity_bindings(domain_id,source_node_id,status,binding_id);

-- A live device public key is an authentication credential identifier. The raw
-- private key remains outside SQLite. One active key cannot bind two local users.
CREATE UNIQUE INDEX user_devices_active_public_key_unique
    ON user_devices(public_key)
    WHERE public_key IS NOT NULL AND status='active';

-- Stable user IDs become the authorization projection for context operations.
-- Historical principal strings remain immutable provenance and request IDs keep
-- their existing principal-scoped uniqueness semantics.
ALTER TABLE context_plans ADD COLUMN actor_user_id TEXT REFERENCES users(user_id);
ALTER TABLE context_operations ADD COLUMN actor_user_id TEXT REFERENCES users(user_id);
ALTER TABLE context_artifacts ADD COLUMN owner_user_id TEXT REFERENCES users(user_id);
ALTER TABLE context_applications ADD COLUMN actor_user_id TEXT REFERENCES users(user_id);
ALTER TABLE context_pending ADD COLUMN actor_user_id TEXT REFERENCES users(user_id);
ALTER TABLE context_source_grants ADD COLUMN principal_user_id TEXT REFERENCES users(user_id);
ALTER TABLE context_derivations ADD COLUMN requester_user_id TEXT REFERENCES users(user_id);

-- Phase 10 deliberately made several context records immutable. Temporarily
-- remove only the affected UPDATE guards inside this migration transaction so
-- stable-user projections can be backfilled, then restore stricter v15 guards.
DROP TRIGGER context_plans_immutable_update;
DROP TRIGGER context_artifacts_immutable_update;
DROP TRIGGER context_applications_immutable_update;
DROP TRIGGER context_derivations_immutable_update;
DROP TRIGGER context_operation_identity_immutable;
DROP TRIGGER context_operation_terminal_immutable;
DROP TRIGGER inherit_context_derivations;

UPDATE context_plans
SET actor_user_id=(SELECT user_id FROM user_identities i WHERE i.external_key=context_plans.actor AND i.status='active' LIMIT 1)
WHERE actor_user_id IS NULL;
UPDATE context_operations
SET actor_user_id=(SELECT user_id FROM user_identities i WHERE i.external_key=context_operations.actor AND i.status='active' LIMIT 1)
WHERE actor_user_id IS NULL;
UPDATE context_artifacts
SET owner_user_id=(SELECT user_id FROM user_identities i WHERE i.external_key=context_artifacts.owner AND i.status='active' LIMIT 1)
WHERE owner_user_id IS NULL;
UPDATE context_applications
SET actor_user_id=(SELECT user_id FROM user_identities i WHERE i.external_key=context_applications.actor AND i.status='active' LIMIT 1)
WHERE actor_user_id IS NULL;
UPDATE context_pending
SET actor_user_id=(SELECT user_id FROM user_identities i WHERE i.external_key=context_pending.actor AND i.status='active' LIMIT 1)
WHERE actor_user_id IS NULL;
UPDATE context_source_grants
SET principal_user_id=(SELECT user_id FROM user_identities i WHERE i.external_key=context_source_grants.principal AND i.status='active' LIMIT 1)
WHERE principal_user_id IS NULL;
UPDATE context_derivations
SET requester_user_id=(SELECT user_id FROM user_identities i WHERE i.external_key=context_derivations.requester AND i.status='active' LIMIT 1)
WHERE requester_user_id IS NULL;

-- Plans and application receipts remain fully immutable after migration.
CREATE TRIGGER context_plans_immutable_update
BEFORE UPDATE ON context_plans
BEGIN SELECT RAISE(ABORT,'plans are immutable'); END;
CREATE TRIGGER context_applications_immutable_update
BEFORE UPDATE ON context_applications
BEGIN SELECT RAISE(ABORT,'application receipts immutable'); END;

-- Artifact/derivation stable-user projections may be learned later from a
-- verified semantic replica. Permit exactly one NULL -> non-NULL enrichment
-- while keeping every historical/provenance field immutable forever.
CREATE TRIGGER context_artifacts_immutable_update
BEFORE UPDATE ON context_artifacts
WHEN NEW.id IS NOT OLD.id
 OR NEW.owner IS NOT OLD.owner
 OR NEW.plan_id IS NOT OLD.plan_id
 OR NEW.operation_id IS NOT OLD.operation_id
 OR NEW.body IS NOT OLD.body
 OR NEW.created_at IS NOT OLD.created_at
 OR OLD.owner_user_id IS NOT NULL
 OR NEW.owner_user_id IS NULL
BEGIN SELECT RAISE(ABORT,'artifacts are immutable except one-time stable-user projection'); END;

CREATE TRIGGER context_derivations_immutable_update
BEFORE UPDATE ON context_derivations
WHEN NEW.node_id IS NOT OLD.node_id
 OR NEW.source_session IS NOT OLD.source_session
 OR NEW.requester IS NOT OLD.requester
 OR NEW.grant_id IS NOT OLD.grant_id
 OR OLD.requester_user_id IS NOT NULL
 OR NEW.requester_user_id IS NULL
BEGIN SELECT RAISE(ABORT,'derivations immutable except one-time stable-user projection'); END;

CREATE TRIGGER inherit_context_derivations
AFTER INSERT ON nodes WHEN NEW.parent_id IS NOT NULL BEGIN
    INSERT OR IGNORE INTO context_derivations(
        node_id,source_session,requester,grant_id,requester_user_id
    )
    SELECT NEW.id,source_session,requester,grant_id,requester_user_id
    FROM context_derivations WHERE node_id=NEW.parent_id;
END;

-- Operation identity now includes its stable actor projection. State settlement
-- remains mutable only from the original started operation as before.
CREATE TRIGGER context_operation_identity_immutable
BEFORE UPDATE ON context_operations
WHEN NEW.id<>OLD.id OR NEW.actor<>OLD.actor OR NEW.request_id<>OLD.request_id OR NEW.plan_id<>OLD.plan_id
 OR NEW.fingerprint<>OLD.fingerprint OR NEW.turn_id<>OLD.turn_id OR NEW.identity_json IS NOT OLD.identity_json
 OR NEW.actor_user_id IS NOT OLD.actor_user_id
BEGIN SELECT RAISE(ABORT,'operation identity immutable'); END;
CREATE TRIGGER context_operation_terminal_immutable
BEFORE UPDATE ON context_operations
WHEN OLD.state<>'started' BEGIN SELECT RAISE(ABORT,'settled operation immutable'); END;

PRAGMA user_version=15;
