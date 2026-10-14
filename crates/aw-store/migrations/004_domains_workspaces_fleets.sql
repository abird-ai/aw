-- Phase 13: first-class Domains, Workspaces, Machines and Fleets.
-- This is an additive control-plane migration. It does NOT enable remote QUIC,
-- direct multi-process ownership or remove owner.lock.

CREATE TABLE domains (
    domain_id TEXT PRIMARY KEY,
    authority_store_id TEXT NOT NULL,
    parent_domain_id TEXT REFERENCES domains(domain_id),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 96),
    legacy_realm TEXT,
    status TEXT NOT NULL CHECK(status IN ('active','suspended','archived')),
    policy_revision INTEGER NOT NULL DEFAULT 1 CHECK(policy_revision>0),
    security_epoch INTEGER NOT NULL DEFAULT 1 CHECK(security_epoch>0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(authority_store_id,parent_domain_id,name)
);
CREATE INDEX domains_by_legacy_realm ON domains(legacy_realm) WHERE legacy_realm IS NOT NULL;

CREATE TABLE domain_memberships (
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    user_id TEXT NOT NULL REFERENCES users(user_id),
    role TEXT NOT NULL CHECK(role IN ('owner','admin','member','viewer')),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    policy_revision INTEGER NOT NULL DEFAULT 1 CHECK(policy_revision>0),
    joined_at INTEGER NOT NULL,
    revoked_at INTEGER,
    PRIMARY KEY(domain_id,user_id)
);
CREATE INDEX domain_memberships_by_user ON domain_memberships(user_id,status,domain_id);

CREATE TABLE machines (
    machine_id TEXT PRIMARY KEY,
    authority_store_id TEXT NOT NULL,
    owning_domain_id TEXT REFERENCES domains(domain_id),
    node_id TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL CHECK(length(display_name) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK(kind IN ('local','remote')),
    key_fingerprint TEXT,
    status TEXT NOT NULL CHECK(status IN ('active','offline','revoked')),
    labels_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(labels_json)),
    capabilities_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(capabilities_json)),
    capability_revision INTEGER NOT NULL DEFAULT 1 CHECK(capability_revision>0),
    security_epoch INTEGER NOT NULL DEFAULT 1 CHECK(security_epoch>0),
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    revoked_at INTEGER
);
CREATE INDEX machines_by_domain ON machines(owning_domain_id,status,machine_id);

CREATE TABLE workspaces (
    workspace_id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK(kind IN ('hosted','fleet_virtual')),
    origin_kind TEXT NOT NULL CHECK(origin_kind IN ('builtin_local','user_created','tool_created','peer_created','fleet_virtual')),
    host_machine_id TEXT REFERENCES machines(machine_id),
    policy_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(policy_json)),
    defaults_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(defaults_json)),
    policy_revision INTEGER NOT NULL DEFAULT 1 CHECK(policy_revision>0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    created_by_user_id TEXT REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    archived_at INTEGER,
    UNIQUE(domain_id,name),
    CHECK((kind='hosted' AND host_machine_id IS NOT NULL) OR kind='fleet_virtual')
);
CREATE INDEX workspaces_by_domain ON workspaces(domain_id,archived_at,name,workspace_id);

CREATE TABLE peer_workspace_origins (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id),
    source_store_id TEXT NOT NULL,
    source_domain_id TEXT NOT NULL,
    source_workspace_id TEXT NOT NULL,
    source_machine_id TEXT,
    source_display_name TEXT,
    created_at INTEGER NOT NULL,
    UNIQUE(source_store_id,source_domain_id,source_workspace_id)
);

CREATE TABLE fleets (
    fleet_id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 128),
    description TEXT,
    membership_revision INTEGER NOT NULL DEFAULT 1 CHECK(membership_revision>0),
    default_execution_policy TEXT NOT NULL DEFAULT 'all' CHECK(default_execution_policy IN ('all','selector','one','any_healthy','quorum')),
    policy_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(policy_json)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(domain_id,name)
);
CREATE INDEX fleets_by_domain ON fleets(domain_id,name,fleet_id);

CREATE TABLE fleet_members (
    fleet_id TEXT NOT NULL REFERENCES fleets(fleet_id),
    machine_id TEXT NOT NULL REFERENCES machines(machine_id),
    state TEXT NOT NULL CHECK(state IN ('active','draining','removed')),
    labels_override_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(labels_override_json)),
    joined_at INTEGER NOT NULL,
    removed_at INTEGER,
    PRIMARY KEY(fleet_id,machine_id)
);

CREATE TABLE fleet_membership_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    fleet_id TEXT NOT NULL REFERENCES fleets(fleet_id),
    membership_revision INTEGER NOT NULL CHECK(membership_revision>0),
    machine_id TEXT NOT NULL REFERENCES machines(machine_id),
    action TEXT NOT NULL CHECK(action IN ('added','draining','removed','reactivated')),
    actor TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TRIGGER fleet_membership_events_immutable_update BEFORE UPDATE ON fleet_membership_events BEGIN SELECT RAISE(ABORT,'fleet membership events immutable'); END;
CREATE TRIGGER fleet_membership_events_immutable_delete BEFORE DELETE ON fleet_membership_events BEGIN SELECT RAISE(ABORT,'fleet membership events immutable'); END;

CREATE TABLE fleet_workspaces (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id),
    fleet_id TEXT NOT NULL REFERENCES fleets(fleet_id),
    default_target_mode TEXT NOT NULL DEFAULT 'fleet' CHECK(default_target_mode IN ('fleet','machine')),
    binding_policy_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(binding_policy_json)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0)
);

CREATE TABLE fleet_workspace_bindings (
    workspace_id TEXT NOT NULL REFERENCES fleet_workspaces(workspace_id),
    machine_id TEXT NOT NULL REFERENCES machines(machine_id),
    remote_workspace_ref TEXT,
    binding_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(binding_json)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(workspace_id,machine_id)
);

ALTER TABLE sessions ADD COLUMN workspace_id TEXT REFERENCES workspaces(workspace_id);

CREATE TABLE session_execution_targets (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id),
    target_kind TEXT NOT NULL CHECK(target_kind IN ('local','machine','fleet')),
    machine_id TEXT REFERENCES machines(machine_id),
    fleet_id TEXT REFERENCES fleets(fleet_id),
    target_snapshot_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(target_snapshot_json)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    updated_at INTEGER NOT NULL,
    CHECK((target_kind='local' AND machine_id IS NOT NULL AND fleet_id IS NULL)
       OR (target_kind='machine' AND machine_id IS NOT NULL AND fleet_id IS NULL)
       OR (target_kind='fleet' AND machine_id IS NULL AND fleet_id IS NOT NULL))
);

-- AwNet/federation metadata is durable while live tunnel connections remain
-- ephemeral. No raw peer secret, provider key or bearer token is stored here.
CREATE TABLE awnet_peers (
    peer_id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL UNIQUE,
    public_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    trust_state TEXT NOT NULL CHECK(trust_state IN ('trusted','revoked')),
    security_epoch INTEGER NOT NULL DEFAULT 1 CHECK(security_epoch>0),
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    revoked_at INTEGER
);

CREATE TABLE domain_federations (
    federation_id TEXT PRIMARY KEY,
    local_domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    remote_peer_id TEXT NOT NULL REFERENCES awnet_peers(peer_id),
    remote_domain_id TEXT NOT NULL,
    direction TEXT NOT NULL CHECK(direction IN ('outbound','inbound')),
    scope_json TEXT NOT NULL CHECK(json_valid(scope_json)),
    permissions_json TEXT NOT NULL CHECK(json_valid(permissions_json)),
    include_child_domains INTEGER NOT NULL DEFAULT 0 CHECK(include_child_domains IN (0,1)),
    allow_redelegation INTEGER NOT NULL DEFAULT 0 CHECK(allow_redelegation IN (0,1)),
    policy_epoch INTEGER NOT NULL DEFAULT 1 CHECK(policy_epoch>0),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX domain_federations_by_domain ON domain_federations(local_domain_id,status,federation_id);

-- Backfill the legacy realm vocabulary into Domains without rewriting historical
-- principal strings. One Domain per legacy realm, one reserved `local` Workspace
-- per Domain on this store's local Machine.
CREATE TEMP TABLE legacy_domain_map(legacy_realm TEXT PRIMARY KEY, domain_id TEXT NOT NULL);
INSERT INTO legacy_domain_map(legacy_realm,domain_id)
SELECT realm_id,'dom_' || lower(hex(randomblob(16))) FROM (SELECT DISTINCT realm_id FROM users);

INSERT INTO domains(domain_id,authority_store_id,name,legacy_realm,status,created_at,updated_at)
SELECT m.domain_id,(SELECT value FROM store_meta WHERE key='store_id'),
       CASE WHEN m.legacy_realm='local' THEN 'Personal' ELSE m.legacy_realm END,
       m.legacy_realm,'active',0,0
FROM legacy_domain_map m;

INSERT INTO domain_memberships(domain_id,user_id,role,status,joined_at)
SELECT m.domain_id,u.user_id,
       CASE WHEN row_number() OVER (PARTITION BY m.domain_id ORDER BY u.created_at,u.user_id)=1 THEN 'owner' ELSE 'member' END,
       'active',0
FROM users u JOIN legacy_domain_map m ON m.legacy_realm=u.realm_id;

CREATE TEMP TABLE local_machine_map(machine_id TEXT NOT NULL,node_id TEXT NOT NULL);
INSERT INTO local_machine_map VALUES('mach_' || lower(hex(randomblob(16))),'node_' || lower(hex(randomblob(16))));
INSERT INTO machines(machine_id,authority_store_id,node_id,display_name,kind,status,created_at,last_seen_at)
SELECT machine_id,(SELECT value FROM store_meta WHERE key='store_id'),node_id,'Local','local','active',0,0 FROM local_machine_map;
INSERT INTO store_meta(key,value) SELECT 'local_machine_id',machine_id FROM local_machine_map;
INSERT INTO store_meta(key,value) SELECT 'local_node_id',node_id FROM local_machine_map;

CREATE TEMP TABLE local_workspace_map(domain_id TEXT PRIMARY KEY,workspace_id TEXT NOT NULL);
INSERT INTO local_workspace_map(domain_id,workspace_id)
SELECT domain_id,'ws_' || lower(hex(randomblob(16))) FROM domains;
INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,host_machine_id,created_by_user_id,created_at,updated_at)
SELECT w.workspace_id,w.domain_id,'local','hosted','builtin_local',(SELECT machine_id FROM local_machine_map),
       (SELECT dm.user_id FROM domain_memberships dm WHERE dm.domain_id=w.domain_id AND dm.role='owner' LIMIT 1),0,0
FROM local_workspace_map w;

UPDATE sessions
SET workspace_id=(
    SELECT lwm.workspace_id
    FROM users u
    JOIN legacy_domain_map dm ON dm.legacy_realm=u.realm_id
    JOIN local_workspace_map lwm ON lwm.domain_id=dm.domain_id
    WHERE u.user_id=sessions.owner_user_id
)
WHERE workspace_id IS NULL;

INSERT INTO session_execution_targets(session_id,target_kind,machine_id,target_snapshot_json,updated_at)
SELECT id,'local',(SELECT machine_id FROM local_machine_map),json_object('legacy_backfill',1),0 FROM sessions;

ALTER TABLE security_audit_events ADD COLUMN domain_id TEXT REFERENCES domains(domain_id);
UPDATE security_audit_events
SET domain_id=(SELECT domain_id FROM legacy_domain_map WHERE legacy_realm=security_audit_events.realm_id)
WHERE domain_id IS NULL AND realm_id IS NOT NULL;

UPDATE store_meta SET value='4' WHERE key='schema_epoch';
UPDATE schema_meta SET schema_epoch=4,activation_epoch=4,updated_at=0 WHERE singleton=1;
UPDATE migration_lease SET current_epoch=4,target_epoch=NULL,updated_at=0 WHERE singleton=1;

DROP TABLE local_workspace_map;
DROP TABLE local_machine_map;
DROP TABLE legacy_domain_map;

PRAGMA user_version=4;
