-- Phase 32: locally verified OIDC login authority and OAuth connector token custody.
-- Raw client secrets/access/refresh tokens remain outside ordinary SQLite.

ALTER TABLE user_identities ADD COLUMN login_verified_at INTEGER;
ALTER TABLE auth_token_families ADD COLUMN oidc_identity_id TEXT REFERENCES user_identities(identity_id);

CREATE TABLE oidc_providers (
    provider_id TEXT PRIMARY KEY,
    issuer TEXT NOT NULL UNIQUE,
    client_id TEXT NOT NULL,
    client_auth_method TEXT NOT NULL CHECK(client_auth_method IN ('none','client_secret_post','client_secret_basic')),
    scopes_json TEXT NOT NULL CHECK(json_valid(scopes_json)),
    status TEXT NOT NULL CHECK(status IN ('active','revoked','unavailable')),
    client_secret_ref TEXT,
    client_secret_version INTEGER,
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    rotated_at INTEGER,
    revoked_at INTEGER,
    CHECK((client_auth_method='none' AND client_secret_ref IS NULL AND client_secret_version IS NULL)
       OR (client_auth_method<>'none' AND client_secret_ref IS NOT NULL AND client_secret_version>0))
);

CREATE TABLE oidc_provider_secret_versions (
    provider_id TEXT NOT NULL REFERENCES oidc_providers(provider_id),
    version INTEGER NOT NULL CHECK(version>0),
    secret_ref TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('active','superseded','revoked','unavailable')),
    created_by_user_id TEXT REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    retired_at INTEGER,
    PRIMARY KEY(provider_id,version)
);

CREATE TABLE oauth_connector_accounts (
    connector_id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(user_id),
    authorization_endpoint TEXT NOT NULL,
    token_endpoint TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_auth_method TEXT NOT NULL CHECK(client_auth_method IN ('none','client_secret_post','client_secret_basic')),
    requested_scopes_json TEXT NOT NULL CHECK(json_valid(requested_scopes_json)),
    granted_scopes_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(granted_scopes_json)),
    status TEXT NOT NULL CHECK(status IN ('configured','active','revoked','unavailable')),
    client_secret_ref TEXT,
    client_secret_version INTEGER,
    token_ref TEXT,
    token_version INTEGER,
    access_expires_at INTEGER,
    created_at INTEGER NOT NULL,
    rotated_at INTEGER,
    revoked_at INTEGER,
    CHECK((client_auth_method='none' AND client_secret_ref IS NULL AND client_secret_version IS NULL)
       OR (client_auth_method<>'none' AND client_secret_ref IS NOT NULL AND client_secret_version>0)),
    CHECK((token_ref IS NULL AND token_version IS NULL) OR (token_ref IS NOT NULL AND token_version>0))
);
CREATE INDEX oauth_connector_owner ON oauth_connector_accounts(owner_user_id,status,connector_id);

CREATE TABLE oauth_connector_client_secret_versions (
    connector_id TEXT NOT NULL REFERENCES oauth_connector_accounts(connector_id),
    version INTEGER NOT NULL CHECK(version>0),
    secret_ref TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('active','superseded','revoked','unavailable')),
    created_at INTEGER NOT NULL,
    retired_at INTEGER,
    PRIMARY KEY(connector_id,version)
);

CREATE TABLE oauth_connector_token_versions (
    connector_id TEXT NOT NULL REFERENCES oauth_connector_accounts(connector_id),
    version INTEGER NOT NULL CHECK(version>0),
    token_ref TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('active','superseded','revoked','unavailable')),
    granted_scopes_json TEXT NOT NULL CHECK(json_valid(granted_scopes_json)),
    access_expires_at INTEGER,
    created_at INTEGER NOT NULL,
    retired_at INTEGER,
    PRIMARY KEY(connector_id,version)
);

-- Pre-Phase-32 OIDC rows were metadata-only. Never promote them to live login authority.
UPDATE user_identities SET login_verified_at=NULL WHERE kind='oidc';

PRAGMA user_version=17;
