-- Immutable checkpoint identity and retained SourceSet; no prepared resource
-- rows. Constructor replay identity belongs to the resulting checkpoint.
CREATE TABLE checkpoints(
    checkpoint_id TEXT PRIMARY KEY,
    source_agent_ref TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    source_branch_id TEXT NOT NULL,
    history_base_id TEXT NOT NULL,
    history_head TEXT NOT NULL,
    source_coverage TEXT NOT NULL CHECK(source_coverage IN ('auto_all_required','selected','none')),
    lifecycle TEXT NOT NULL CHECK(lifecycle IN ('live','retired')),
    note TEXT CHECK(note IS NULL OR length(CAST(note AS BLOB)) BETWEEN 1 AND 2048),
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    request_id TEXT NOT NULL CHECK(length(CAST(request_id AS BLOB)) BETWEEN 1 AND 256),
    request_fingerprint TEXT NOT NULL CHECK(length(request_fingerprint)=64),
    -- A tombstone keeps its original history identity but is not a GC root.
    live_history_base_id TEXT GENERATED ALWAYS AS (
        CASE WHEN lifecycle='live' THEN history_base_id END
    ) STORED REFERENCES history_bases(base_id) ON DELETE RESTRICT,
    FOREIGN KEY(source_session_id,source_branch_id)
        REFERENCES agent_state_current(session_id,branch_id) ON DELETE RESTRICT,
    UNIQUE(created_by,request_id)
);
CREATE INDEX checkpoints_by_agent ON checkpoints(source_session_id,source_branch_id,created_at,checkpoint_id);
CREATE INDEX checkpoints_live_history ON checkpoints(live_history_base_id) WHERE live_history_base_id IS NOT NULL;
CREATE TRIGGER checkpoints_immutable_update
BEFORE UPDATE ON checkpoints
WHEN OLD.lifecycle<>'live' OR NEW.lifecycle<>'retired'
    OR NEW.checkpoint_id IS NOT OLD.checkpoint_id
    OR NEW.source_agent_ref IS NOT OLD.source_agent_ref
    OR NEW.source_session_id IS NOT OLD.source_session_id
    OR NEW.source_branch_id IS NOT OLD.source_branch_id
    OR NEW.history_base_id IS NOT OLD.history_base_id
    OR NEW.history_head IS NOT OLD.history_head
    OR NEW.source_coverage IS NOT OLD.source_coverage
    OR NEW.note IS NOT OLD.note
    OR NEW.created_by IS NOT OLD.created_by
    OR NEW.created_at IS NOT OLD.created_at
    OR NEW.request_id IS NOT OLD.request_id
    OR NEW.request_fingerprint IS NOT OLD.request_fingerprint
BEGIN SELECT RAISE(ABORT,'checkpoint identity is immutable'); END;
CREATE TRIGGER checkpoints_replay_retained
BEFORE DELETE ON checkpoints
BEGIN SELECT RAISE(ABORT,'checkpoint replay identity is retained'); END;

CREATE TABLE checkpoint_sources(
    checkpoint_id TEXT NOT NULL REFERENCES checkpoints(checkpoint_id) ON DELETE RESTRICT,
    source_name TEXT NOT NULL CHECK(length(source_name) BETWEEN 1 AND 64
        AND source_name GLOB '[a-z]*' AND source_name NOT GLOB '*[^a-z0-9_-]*'),
    driver_key TEXT NOT NULL CHECK(length(CAST(driver_key AS BLOB)) BETWEEN 1 AND 256),
    anchor_schema INTEGER NOT NULL CHECK(anchor_schema BETWEEN 1 AND 4294967295),
    source_ref TEXT NOT NULL CHECK(length(CAST(source_ref AS BLOB)) BETWEEN 1 AND 2048),
    captured_access TEXT NOT NULL CHECK(captured_access IN ('ro','rw')),
    captured_checkpoint_policy TEXT NOT NULL CHECK(captured_checkpoint_policy IN ('required','ephemeral')),
    anchor_identity TEXT NOT NULL CHECK(length(CAST(anchor_identity AS BLOB)) BETWEEN 1 AND 2048),
    retention TEXT NOT NULL CHECK(length(CAST(retention AS BLOB)) BETWEEN 1 AND 2048),
    anchor_fields_json TEXT NOT NULL CHECK(json_valid(anchor_fields_json)
        AND length(CAST(anchor_fields_json AS BLOB)) BETWEEN 2 AND 262144),
    compatibility TEXT NOT NULL CHECK(compatibility IN ('compatible','recovery_required')),
    PRIMARY KEY(checkpoint_id,source_name)
) WITHOUT ROWID;
CREATE TRIGGER checkpoint_sources_immutable_update
BEFORE UPDATE OF checkpoint_id,source_name,driver_key,anchor_schema,source_ref,
    captured_access,captured_checkpoint_policy,anchor_identity,retention,anchor_fields_json
ON checkpoint_sources
BEGIN SELECT RAISE(ABORT,'checkpoint source anchors are immutable'); END;
CREATE TRIGGER checkpoint_sources_live_retention
BEFORE DELETE ON checkpoint_sources
WHEN EXISTS(SELECT 1 FROM checkpoints WHERE checkpoint_id=OLD.checkpoint_id AND lifecycle='live')
BEGIN SELECT RAISE(ABORT,'live checkpoint retains its source anchors'); END;
