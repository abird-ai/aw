-- DL-200 Phase 1: immutable structurally shared history/disclosure owner.
-- Existing Session nodes remain the physical local journal; sealing is lazy.

CREATE TABLE disclosure_authority_roots(
    root_digest TEXT PRIMARY KEY CHECK(length(root_digest)=64),
    kind TEXT NOT NULL,
    stable_key TEXT NOT NULL,
    evidence_json TEXT NOT NULL CHECK(json_valid(evidence_json)),
    created_at INTEGER NOT NULL,
    UNIQUE(kind,stable_key,evidence_json)
);
CREATE TRIGGER disclosure_authority_roots_immutable_update
BEFORE UPDATE ON disclosure_authority_roots
BEGIN SELECT RAISE(ABORT,'disclosure authority roots are immutable'); END;

CREATE TABLE disclosure_manifest_nodes(
    node_digest TEXT PRIMARY KEY CHECK(length(node_digest)=64),
    node_kind TEXT NOT NULL CHECK(node_kind IN ('leaf','branch')),
    item_count INTEGER NOT NULL CHECK(item_count>=0),
    created_at INTEGER NOT NULL
);
CREATE TRIGGER disclosure_manifest_nodes_immutable_update
BEFORE UPDATE ON disclosure_manifest_nodes
BEGIN SELECT RAISE(ABORT,'disclosure manifest nodes are immutable'); END;

CREATE TABLE disclosure_manifest_entries(
    node_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    entry_kind TEXT NOT NULL CHECK(entry_kind IN ('fact','authority_root')),
    entry_ref TEXT NOT NULL,
    PRIMARY KEY(node_digest,ordinal),
    UNIQUE(node_digest,entry_kind,entry_ref)
);
CREATE TRIGGER disclosure_manifest_entries_immutable_update
BEFORE UPDATE ON disclosure_manifest_entries
BEGIN SELECT RAISE(ABORT,'disclosure manifest entries are immutable'); END;

CREATE TABLE disclosure_manifest_children(
    node_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    child_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE RESTRICT,
    PRIMARY KEY(node_digest,ordinal),
    UNIQUE(node_digest,child_digest)
);
CREATE TRIGGER disclosure_manifest_children_immutable_update
BEFORE UPDATE ON disclosure_manifest_children
BEGIN SELECT RAISE(ABORT,'disclosure manifest children are immutable'); END;

CREATE TABLE history_spans(
    span_id TEXT PRIMARY KEY,
    seal_key TEXT NOT NULL UNIQUE CHECK(length(seal_key)=64),
    source_agent_ref TEXT NOT NULL,
    backing_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE RESTRICT,
    backing_branch_id TEXT NOT NULL REFERENCES branches(id) ON DELETE RESTRICT,
    local_root_node TEXT NOT NULL,
    exact_head_node TEXT NOT NULL,
    content_digest TEXT NOT NULL CHECK(length(content_digest)=64),
    provenance_digest TEXT NOT NULL CHECK(length(provenance_digest)=64),
    manifest_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE RESTRICT,
    total_turns INTEGER NOT NULL CHECK(total_turns>=0),
    next_turn INTEGER NOT NULL CHECK(next_turn>=1),
    created_at INTEGER NOT NULL,
    FOREIGN KEY(backing_session_id,local_root_node) REFERENCES nodes(session_id,id) ON DELETE RESTRICT,
    FOREIGN KEY(backing_session_id,exact_head_node) REFERENCES nodes(session_id,id) ON DELETE RESTRICT
);
CREATE INDEX history_spans_backing
    ON history_spans(backing_session_id,backing_branch_id,exact_head_node);
CREATE TRIGGER history_spans_immutable_update
BEFORE UPDATE ON history_spans
BEGIN SELECT RAISE(ABORT,'history spans are immutable'); END;

CREATE TABLE history_span_nodes(
    span_id TEXT NOT NULL REFERENCES history_spans(span_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    node_id TEXT NOT NULL,
    node_digest TEXT NOT NULL CHECK(length(node_digest)=64),
    PRIMARY KEY(span_id,ordinal),
    UNIQUE(span_id,node_id)
);
CREATE TRIGGER history_span_nodes_immutable_update
BEFORE UPDATE ON history_span_nodes
BEGIN SELECT RAISE(ABORT,'history span node links are immutable'); END;

CREATE TABLE history_bases(
    base_id TEXT PRIMARY KEY,
    seal_key TEXT NOT NULL UNIQUE CHECK(length(seal_key)=64),
    parent_base_id TEXT REFERENCES history_bases(base_id) ON DELETE RESTRICT,
    local_span_id TEXT NOT NULL REFERENCES history_spans(span_id) ON DELETE RESTRICT,
    total_turns INTEGER NOT NULL CHECK(total_turns>=0),
    next_turn INTEGER NOT NULL CHECK(next_turn>=1),
    history_content_digest TEXT NOT NULL CHECK(length(history_content_digest)=64),
    manifest_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL
);
CREATE INDEX history_bases_parent ON history_bases(parent_base_id,base_id);
CREATE TRIGGER history_bases_immutable_update
BEFORE UPDATE ON history_bases
BEGIN SELECT RAISE(ABORT,'history bases are immutable'); END;

CREATE TABLE history_grants(
    grant_id TEXT PRIMARY KEY,
    base_id TEXT NOT NULL REFERENCES history_bases(base_id) ON DELETE RESTRICT,
    source_agent_ref TEXT NOT NULL,
    destination_agent_ref TEXT NOT NULL,
    manifest_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest) ON DELETE RESTRICT,
    created_by_handle TEXT NOT NULL,
    supersedes_grant_id TEXT REFERENCES history_grants(grant_id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL
);
CREATE INDEX history_grants_destination ON history_grants(destination_agent_ref,created_at,grant_id);
CREATE TRIGGER history_grants_immutable_update
BEFORE UPDATE ON history_grants
BEGIN SELECT RAISE(ABORT,'history grants are immutable'); END;

CREATE TABLE history_grant_principals(
    grant_id TEXT NOT NULL REFERENCES history_grants(grant_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    principal_kind TEXT NOT NULL CHECK(principal_kind IN ('user','group')),
    principal_handle TEXT NOT NULL,
    PRIMARY KEY(grant_id,ordinal),
    UNIQUE(grant_id,principal_kind,principal_handle)
);
CREATE TRIGGER history_grant_principals_immutable_update
BEFORE UPDATE ON history_grant_principals
BEGIN SELECT RAISE(ABORT,'history grant principals are immutable'); END;
