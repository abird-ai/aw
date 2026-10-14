-- Phase 22: short-lived Domain federation capability leases.

CREATE TABLE federation_capability_leases (
    lease_id TEXT PRIMARY KEY,
    federation_id TEXT NOT NULL REFERENCES domain_federations(federation_id),
    remote_peer_id TEXT NOT NULL REFERENCES awnet_peers(peer_id),
    issued_to_node_id TEXT NOT NULL,
    scope_json TEXT NOT NULL CHECK(json_valid(scope_json)),
    permissions_json TEXT NOT NULL CHECK(json_valid(permissions_json)),
    policy_epoch INTEGER NOT NULL CHECK(policy_epoch>0),
    domain_policy_revision INTEGER NOT NULL CHECK(domain_policy_revision>0),
    domain_security_epoch INTEGER NOT NULL CHECK(domain_security_epoch>0),
    peer_security_epoch INTEGER NOT NULL CHECK(peer_security_epoch>0),
    issued_by TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
);
CREATE INDEX federation_capability_leases_active
    ON federation_capability_leases(federation_id,status,expires_at,lease_id);
CREATE INDEX federation_capability_leases_peer
    ON federation_capability_leases(remote_peer_id,status,expires_at,lease_id);

PRAGMA user_version=7;
