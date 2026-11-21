-- DL-200 Phase 1: one principal namespace per Domain plus simple user-set groups.

CREATE TABLE domain_principals(
    domain_id TEXT NOT NULL REFERENCES domains(domain_id) ON DELETE CASCADE,
    principal_handle TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('user','group')),
    user_id TEXT REFERENCES users(user_id) ON DELETE RESTRICT,
    status TEXT NOT NULL CHECK(status IN ('active','deleted')),
    display_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(display_json)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(domain_id,principal_handle),
    UNIQUE(domain_id,user_id),
    CHECK((kind='user' AND user_id IS NOT NULL) OR (kind='group' AND user_id IS NULL))
);
CREATE INDEX domain_principals_by_kind
    ON domain_principals(domain_id,status,kind,principal_handle);

CREATE TABLE principal_groups(
    domain_id TEXT NOT NULL,
    principal_handle TEXT NOT NULL,
    owner_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE RESTRICT,
    rev INTEGER NOT NULL CHECK(rev>=1),
    deleted_at INTEGER,
    PRIMARY KEY(domain_id,principal_handle),
    FOREIGN KEY(domain_id,principal_handle)
        REFERENCES domain_principals(domain_id,principal_handle) ON DELETE RESTRICT
);

CREATE TABLE principal_group_members(
    domain_id TEXT NOT NULL,
    group_handle TEXT NOT NULL,
    member_handle TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active','removed')),
    revision INTEGER NOT NULL CHECK(revision>=1),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(domain_id,group_handle,member_handle),
    FOREIGN KEY(domain_id,group_handle)
        REFERENCES principal_groups(domain_id,principal_handle) ON DELETE CASCADE,
    FOREIGN KEY(domain_id,member_handle)
        REFERENCES domain_principals(domain_id,principal_handle) ON DELETE RESTRICT,
    CHECK(group_handle<>member_handle)
);
CREATE INDEX principal_group_members_active
    ON principal_group_members(domain_id,member_handle,group_handle,revision)
    WHERE state='active';

CREATE TRIGGER domain_membership_principal_collision
BEFORE INSERT ON domain_memberships
WHEN EXISTS(
    SELECT 1 FROM domain_principals p
    WHERE p.domain_id=NEW.domain_id AND p.principal_handle=NEW.user_id
      AND NOT (p.kind='user' AND p.user_id=NEW.user_id)
)
BEGIN
    SELECT RAISE(ABORT,'domain principal handle collision');
END;

CREATE TRIGGER domain_membership_principal_register
AFTER INSERT ON domain_memberships
BEGIN
    INSERT INTO domain_principals(
        domain_id,principal_handle,kind,user_id,status,display_json,created_at,updated_at
    ) VALUES(NEW.domain_id,NEW.user_id,'user',NEW.user_id,'active','{}',NEW.joined_at,NEW.joined_at)
    ON CONFLICT(domain_id,principal_handle) DO UPDATE SET
        status='active',updated_at=excluded.updated_at
    WHERE domain_principals.kind='user' AND domain_principals.user_id=excluded.user_id;
END;
