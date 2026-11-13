-- Phase 5 unified runtime: explicit shared-Session participation authority.
-- Legacy members.can_append remains untouched and continues to guard only the
-- legacy Session append/read APIs. These rows never grant tools/providers/payer.

CREATE TABLE session_participation_state(
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    audience_epoch INTEGER NOT NULL CHECK(audience_epoch>=1),
    management_revision INTEGER NOT NULL CHECK(management_revision>=1),
    updated_at INTEGER NOT NULL
);

INSERT INTO session_participation_state(session_id,audience_epoch,management_revision,updated_at)
SELECT id,1,1,updated_at FROM sessions;

CREATE TRIGGER session_participation_state_on_session_insert
AFTER INSERT ON sessions
BEGIN
    INSERT INTO session_participation_state(
        session_id,audience_epoch,management_revision,updated_at
    ) VALUES(NEW.id,1,1,NEW.updated_at);
END;

CREATE TABLE session_participation_grants(
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    can_view INTEGER NOT NULL CHECK(can_view IN (0,1)),
    can_contribute INTEGER NOT NULL CHECK(can_contribute IN (0,1)),
    can_submit INTEGER NOT NULL CHECK(can_submit IN (0,1)),
    can_control INTEGER NOT NULL CHECK(can_control IN (0,1)),
    can_manage INTEGER NOT NULL CHECK(can_manage IN (0,1)),
    status TEXT NOT NULL CHECK(status IN ('active','revoked')),
    revision INTEGER NOT NULL CHECK(revision>=1),
    granted_by_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE RESTRICT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(session_id,user_id),
    CHECK(
        (status='active' AND can_view=1)
        OR
        (status='revoked' AND can_view=0 AND can_contribute=0
         AND can_submit=0 AND can_control=0 AND can_manage=0)
    ),
    CHECK(can_view=1 OR (can_contribute=0 AND can_submit=0 AND can_control=0 AND can_manage=0))
);

CREATE INDEX session_participation_active
    ON session_participation_grants(session_id,user_id,revision)
    WHERE status='active';

UPDATE store_meta SET value='34' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=34,activation_epoch=34,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=34,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=34;
