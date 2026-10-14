-- Phase 27: semantic Domain authority-core snapshots and quorum-fenced standby promotion.
-- Standby snapshots are immutable evidence. Promotion materializes only the
-- minimal authority core (Domain metadata, safe User metadata, Domain memberships,
-- AuthoritySet and quorum proof), never raw SQLite state or credentials.

CREATE TABLE domain_authority_replica_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    authority_domain_id TEXT NOT NULL,
    source_store_id TEXT NOT NULL,
    source_node_id TEXT NOT NULL,
    set_revision INTEGER NOT NULL CHECK(set_revision>0),
    members_sha256 TEXT NOT NULL CHECK(length(members_sha256)=64),
    quorum INTEGER NOT NULL CHECK(quorum>0),
    authority_term INTEGER NOT NULL CHECK(authority_term>=0),
    through_commit_seq INTEGER NOT NULL CHECK(through_commit_seq>=0),
    snapshot_sha256 TEXT NOT NULL CHECK(length(snapshot_sha256)=64),
    body_json TEXT NOT NULL CHECK(json_valid(body_json)),
    source_signature TEXT NOT NULL,
    imported_by_grant_id TEXT NOT NULL REFERENCES domain_authority_peer_grants(grant_id),
    imported_at INTEGER NOT NULL,
    UNIQUE(authority_domain_id,source_store_id,through_commit_seq),
    UNIQUE(authority_domain_id,snapshot_sha256)
);
CREATE INDEX domain_authority_replica_snapshots_domain
    ON domain_authority_replica_snapshots(authority_domain_id,through_commit_seq,snapshot_id);
CREATE TRIGGER domain_authority_replica_snapshots_immutable_update
BEFORE UPDATE ON domain_authority_replica_snapshots
BEGIN SELECT RAISE(ABORT,'Domain authority replica snapshot immutable'); END;
CREATE TRIGGER domain_authority_replica_snapshots_immutable_delete
BEFORE DELETE ON domain_authority_replica_snapshots
BEGIN SELECT RAISE(ABORT,'Domain authority replica snapshot immutable'); END;

CREATE TABLE domain_authority_replica_heads (
    authority_domain_id TEXT PRIMARY KEY,
    snapshot_id TEXT NOT NULL REFERENCES domain_authority_replica_snapshots(snapshot_id),
    source_store_id TEXT NOT NULL,
    set_revision INTEGER NOT NULL CHECK(set_revision>0),
    members_sha256 TEXT NOT NULL CHECK(length(members_sha256)=64),
    through_commit_seq INTEGER NOT NULL CHECK(through_commit_seq>=0),
    status TEXT NOT NULL CHECK(status IN ('standby','promoted','superseded')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
    updated_at INTEGER NOT NULL
);

CREATE TABLE domain_authority_replica_elections (
    election_id TEXT PRIMARY KEY,
    authority_domain_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL REFERENCES domain_authority_replica_snapshots(snapshot_id),
    set_revision INTEGER NOT NULL CHECK(set_revision>0),
    members_sha256 TEXT NOT NULL CHECK(length(members_sha256)=64),
    quorum INTEGER NOT NULL CHECK(quorum>0),
    term INTEGER NOT NULL CHECK(term>0),
    candidate_node_id TEXT NOT NULL,
    requested_ttl_seconds INTEGER NOT NULL CHECK(requested_ttl_seconds BETWEEN 5 AND 300),
    state TEXT NOT NULL CHECK(state IN ('collecting','active','superseded','expired','promoted')),
    created_by_user_id TEXT NOT NULL REFERENCES users(user_id),
    created_at INTEGER NOT NULL,
    activated_at INTEGER,
    expires_at INTEGER,
    UNIQUE(authority_domain_id,term)
);
CREATE INDEX domain_authority_replica_elections_current
    ON domain_authority_replica_elections(authority_domain_id,state,term);

CREATE TABLE domain_authority_replica_votes (
    election_id TEXT NOT NULL REFERENCES domain_authority_replica_elections(election_id),
    voter_node_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    signature TEXT,
    received_at INTEGER NOT NULL,
    PRIMARY KEY(election_id,voter_node_id)
);
CREATE TRIGGER domain_authority_replica_votes_immutable_update
BEFORE UPDATE ON domain_authority_replica_votes
BEGIN SELECT RAISE(ABORT,'Domain authority replica vote immutable'); END;
CREATE TRIGGER domain_authority_replica_votes_immutable_delete
BEFORE DELETE ON domain_authority_replica_votes
BEGIN SELECT RAISE(ABORT,'Domain authority replica vote immutable'); END;

CREATE TABLE domain_authority_promotions (
    promotion_id TEXT PRIMARY KEY,
    authority_domain_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL REFERENCES domain_authority_replica_snapshots(snapshot_id),
    election_id TEXT NOT NULL REFERENCES domain_authority_replica_elections(election_id),
    previous_authority_store_id TEXT NOT NULL,
    promoted_authority_store_id TEXT NOT NULL,
    promoted_node_id TEXT NOT NULL,
    promoted_term INTEGER NOT NULL CHECK(promoted_term>0),
    leader_expires_at INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active','superseded')),
    promoted_at INTEGER NOT NULL,
    UNIQUE(authority_domain_id,promoted_term)
);
CREATE INDEX domain_authority_promotions_current
    ON domain_authority_promotions(authority_domain_id,status,promoted_term);

PRAGMA user_version=12;
