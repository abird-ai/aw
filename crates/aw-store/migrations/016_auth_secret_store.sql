-- Phase 31: native auth-session rotation and shared/server SecretStore metadata.
-- Raw secret bytes remain outside ordinary SQLite plaintext.

CREATE TABLE auth_token_families (
    family_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    credential_id TEXT REFERENCES local_auth_credentials(credential_id),
    portable_binding_id TEXT REFERENCES portable_identity_bindings(binding_id),
    auth_method TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    current_session_id TEXT,
    security_epoch_at_issue INTEGER NOT NULL CHECK(security_epoch_at_issue>0),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_rotated_at INTEGER,
    reuse_detected_at INTEGER,
    revoked_at INTEGER,
    client_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(client_json))
);
CREATE INDEX auth_token_families_user ON auth_token_families(user_id,status,expires_at,family_id);
CREATE UNIQUE INDEX auth_sessions_refresh_hash_unique ON auth_sessions(refresh_token_hash);
ALTER TABLE auth_sessions ADD COLUMN rotated_to_session_id TEXT;
ALTER TABLE auth_sessions ADD COLUMN rotated_at INTEGER;
CREATE INDEX auth_sessions_family ON auth_sessions(rotation_family_id,issued_at,session_id);
-- auth_sessions existed as dormant metadata before Phase 31. Fail closed on
-- upgrade: no pre-v16 bearer token is promoted into a live rotating family.
UPDATE auth_sessions SET revoked_at=COALESCE(revoked_at,issued_at);

CREATE TABLE provider_account_grants (
    provider_account_id TEXT NOT NULL REFERENCES provider_accounts(provider_account_id),
    user_id TEXT NOT NULL REFERENCES users(user_id),
    role TEXT NOT NULL CHECK(role IN ('use','admin')),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    created_at INTEGER NOT NULL,
    revoked_at INTEGER,
    PRIMARY KEY(provider_account_id,user_id)
);
CREATE INDEX provider_account_grants_user
    ON provider_account_grants(user_id,status,provider_account_id);

CREATE TABLE provider_secret_versions (
    provider_account_id TEXT NOT NULL REFERENCES provider_accounts(provider_account_id),
    version INTEGER NOT NULL CHECK(version>0),
    secret_ref TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','superseded','revoked','unavailable')),
    created_by_user_id TEXT REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    retired_at INTEGER,
    PRIMARY KEY(provider_account_id,version),
    UNIQUE(secret_ref)
);
CREATE INDEX provider_secret_versions_status
    ON provider_secret_versions(provider_account_id,status,version);

-- Old schema3 server-secret rows were metadata-only placeholders. Never assume
-- their referenced secret exists after upgrading to the real SecretStore plane.
UPDATE provider_accounts
SET secret_version=CASE WHEN secret_version IS NULL OR secret_version<=0 THEN 1 ELSE secret_version END,
    status=CASE WHEN status='revoked' THEN 'revoked' ELSE 'unavailable' END
WHERE credential_custody='server_secret';
INSERT OR IGNORE INTO provider_secret_versions(
    provider_account_id,version,secret_ref,status,created_at
)
SELECT provider_account_id,secret_version,secret_ref,CASE WHEN status='revoked' THEN 'revoked' ELSE 'unavailable' END,created_at
FROM provider_accounts
WHERE credential_custody='server_secret' AND secret_ref IS NOT NULL AND secret_version IS NOT NULL;
UPDATE provider_accounts SET secret_version=NULL WHERE credential_custody<>'server_secret';

CREATE TRIGGER provider_accounts_secret_shape_insert
BEFORE INSERT ON provider_accounts
WHEN (NEW.credential_custody='server_secret' AND (NEW.secret_ref IS NULL OR NEW.secret_version IS NULL OR NEW.secret_version<=0))
  OR (NEW.credential_custody<>'server_secret' AND (NEW.secret_ref IS NOT NULL OR NEW.secret_version IS NOT NULL))
BEGIN SELECT RAISE(ABORT,'provider account secret metadata/custody mismatch'); END;

CREATE TRIGGER provider_accounts_secret_shape_update
BEFORE UPDATE ON provider_accounts
WHEN (NEW.credential_custody='server_secret' AND (NEW.secret_ref IS NULL OR NEW.secret_version IS NULL OR NEW.secret_version<=0))
  OR (NEW.credential_custody<>'server_secret' AND (NEW.secret_ref IS NOT NULL OR NEW.secret_version IS NOT NULL))
BEGIN SELECT RAISE(ABORT,'provider account secret metadata/custody mismatch'); END;

PRAGMA user_version=16;
