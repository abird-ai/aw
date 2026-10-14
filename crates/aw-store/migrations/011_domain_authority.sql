-- Phase 26: Domain AuthoritySet, quorum votes and bounded leader leases.
-- This is a control-plane fencing primitive. It does not create generic DB
-- replication or remove the local SQLite owner.

CREATE TABLE domain_authority_sets (
    domain_id TEXT PRIMARY KEY REFERENCES domains(domain_id),
    set_revision INTEGER NOT NULL CHECK(set_revision>0),
    quorum INTEGER NOT NULL CHECK(quorum>0),
    members_sha256 TEXT NOT NULL CHECK(length(members_sha256)=64),
    current_term INTEGER NOT NULL DEFAULT 0 CHECK(current_term>=0),
    updated_at INTEGER NOT NULL
);

CREATE TABLE domain_authority_members (
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    node_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    state TEXT NOT NULL CHECK(state IN ('active','removed')),
    joined_at INTEGER NOT NULL,
    removed_at INTEGER,
    PRIMARY KEY(domain_id,node_id)
);
CREATE INDEX domain_authority_members_active
    ON domain_authority_members(domain_id,state,ordinal,node_id);

CREATE TABLE domain_authority_elections (
    election_id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES domains(domain_id),
    set_revision INTEGER NOT NULL CHECK(set_revision>0),
    members_sha256 TEXT NOT NULL CHECK(length(members_sha256)=64),
    quorum INTEGER NOT NULL CHECK(quorum>0),
    term INTEGER NOT NULL CHECK(term>0),
    candidate_node_id TEXT NOT NULL,
    requested_ttl_seconds INTEGER NOT NULL CHECK(requested_ttl_seconds BETWEEN 5 AND 300),
    state TEXT NOT NULL CHECK(state IN ('collecting','active','superseded','expired')),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    activated_at INTEGER,
    expires_at INTEGER,
    UNIQUE(domain_id,term)
);
CREATE INDEX domain_authority_elections_current
    ON domain_authority_elections(domain_id,state,term);

CREATE TABLE domain_authority_votes (
    election_id TEXT NOT NULL REFERENCES domain_authority_elections(election_id),
    voter_node_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    signature TEXT,
    received_at INTEGER NOT NULL,
    PRIMARY KEY(election_id,voter_node_id)
);
CREATE TRIGGER domain_authority_votes_immutable_update
BEFORE UPDATE ON domain_authority_votes
BEGIN SELECT RAISE(ABORT,'domain authority vote immutable'); END;
CREATE TRIGGER domain_authority_votes_immutable_delete
BEFORE DELETE ON domain_authority_votes
BEGIN SELECT RAISE(ABORT,'domain authority vote immutable'); END;

-- A remote authority voter explicitly accepts one foreign Domain AuthoritySet.
-- The foreign Domain need not be a local canonical Domain replica yet.
CREATE TABLE domain_authority_peer_grants (
    grant_id TEXT PRIMARY KEY,
    authority_domain_id TEXT NOT NULL,
    accepted_from_node_id TEXT NOT NULL,
    set_revision INTEGER NOT NULL CHECK(set_revision>0),
    members_sha256 TEXT NOT NULL CHECK(length(members_sha256)=64),
    quorum INTEGER NOT NULL CHECK(quorum>0),
    members_json TEXT NOT NULL CHECK(json_valid(members_json)),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    accepted_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    accepted_at INTEGER NOT NULL,
    revoked_at INTEGER,
    UNIQUE(authority_domain_id,set_revision,members_sha256)
);
CREATE INDEX domain_authority_peer_grants_active
    ON domain_authority_peer_grants(authority_domain_id,status,set_revision);

-- Votes cast by this node for foreign Domain elections. One row per term/voter
-- prevents divergent double-votes. Unexpired votes also fence overlapping
-- candidates across later terms.
CREATE TABLE domain_authority_peer_votes (
    authority_domain_id TEXT NOT NULL,
    set_revision INTEGER NOT NULL CHECK(set_revision>0),
    members_sha256 TEXT NOT NULL CHECK(length(members_sha256)=64),
    term INTEGER NOT NULL CHECK(term>0),
    election_id TEXT NOT NULL,
    voter_node_id TEXT NOT NULL,
    candidate_node_id TEXT NOT NULL,
    ttl_seconds INTEGER NOT NULL CHECK(ttl_seconds BETWEEN 5 AND 300),
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(authority_domain_id,set_revision,term,voter_node_id),
    UNIQUE(authority_domain_id,election_id,voter_node_id)
);
CREATE TRIGGER domain_authority_peer_votes_immutable_update
BEFORE UPDATE ON domain_authority_peer_votes
BEGIN SELECT RAISE(ABORT,'domain authority peer vote immutable'); END;
CREATE TRIGGER domain_authority_peer_votes_immutable_delete
BEFORE DELETE ON domain_authority_peer_votes
BEGIN SELECT RAISE(ABORT,'domain authority peer vote immutable'); END;

PRAGMA user_version=11;
