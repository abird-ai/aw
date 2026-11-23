-- DL-200 Phase 2: generalize opaque Event read placement from exact Agent
-- to one of the reviewed same-store placement scopes. Existing cursors remain
-- exact-Agent cursors.

ALTER TABLE event_read_cursors_current
    RENAME COLUMN agent_ref TO placement_ref;

ALTER TABLE event_read_cursors_current
    ADD COLUMN placement_kind TEXT NOT NULL DEFAULT 'agent'
    CHECK(placement_kind IN ('agent','project'));
