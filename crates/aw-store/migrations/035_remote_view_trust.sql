-- Workspace views remote-device trust: durable, read-only grants that survive
-- transport reconnect but not identity/revision/security changes.
--
-- Pairing/MachineLink is transport identity only. A remote view additionally
-- requires a stable authenticated device/user, active Domain membership,
-- an explicitly registered requester Machine, and one exact hosted Workspace.
-- No PTY/process/provider/tool authority is representable in this table.

CREATE TABLE remote_view_grants(
    grant_id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES domains(domain_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    workspace_revision INTEGER NOT NULL CHECK(workspace_revision>=1),
    workspace_policy_revision INTEGER NOT NULL CHECK(workspace_policy_revision>=1),

    target_machine_id TEXT NOT NULL REFERENCES machines(machine_id) ON DELETE RESTRICT,
    target_machine_node_id TEXT NOT NULL,
    target_machine_security_epoch INTEGER NOT NULL CHECK(target_machine_security_epoch>=1),
    target_machine_capability_revision INTEGER NOT NULL CHECK(target_machine_capability_revision>=1),

    requester_machine_id TEXT NOT NULL REFERENCES machines(machine_id) ON DELETE RESTRICT,
    requester_machine_node_id TEXT NOT NULL,
    requester_machine_security_epoch INTEGER NOT NULL CHECK(requester_machine_security_epoch>=1),
    requester_machine_capability_revision INTEGER NOT NULL CHECK(requester_machine_capability_revision>=1),

    peer_id TEXT NOT NULL REFERENCES awnet_peers(peer_id) ON DELETE RESTRICT,
    peer_node_id TEXT NOT NULL,
    peer_security_epoch INTEGER NOT NULL CHECK(peer_security_epoch>=1),
    peer_public_key TEXT NOT NULL,

    user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE RESTRICT,
    stable_principal TEXT NOT NULL,
    user_security_epoch INTEGER NOT NULL CHECK(user_security_epoch>=1),
    domain_policy_revision INTEGER NOT NULL CHECK(domain_policy_revision>=1),
    domain_security_epoch INTEGER NOT NULL CHECK(domain_security_epoch>=1),

    device_binding_id TEXT NOT NULL REFERENCES portable_identity_bindings(binding_id) ON DELETE RESTRICT,
    device_id TEXT NOT NULL,
    device_public_key TEXT NOT NULL,

    permissions_json TEXT NOT NULL CHECK(json_valid(permissions_json)),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    revision INTEGER NOT NULL CHECK(revision>=1),
    issued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    revoked_at INTEGER,
    CHECK(expires_at>issued_at),
    CHECK((status='active' AND revoked_at IS NULL) OR status='revoked')
);

CREATE INDEX remote_view_grants_active_user
    ON remote_view_grants(user_id,workspace_id,status,expires_at);
CREATE INDEX remote_view_grants_active_peer
    ON remote_view_grants(peer_node_id,requester_machine_id,status,expires_at);

UPDATE store_meta SET value='35' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=35,activation_epoch=35,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=35,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=35;
