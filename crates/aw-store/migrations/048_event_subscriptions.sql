-- DL-200 Phase 2: durable Event subscription owner.
-- Backlog stays in immutable Event history; rows own only private scan progress.

CREATE TABLE event_subscriptions_current(
    recipient_ref TEXT NOT NULL CHECK(length(recipient_ref) BETWEEN 1 AND 2048),
    subject TEXT NOT NULL CHECK(length(subject) BETWEEN 1 AND 2048),
    placement_kind TEXT NOT NULL CHECK(placement_kind IN ('agent','project')),
    placement_ref TEXT NOT NULL CHECK(length(placement_ref) BETWEEN 1 AND 2048),
    placement_token BLOB NOT NULL CHECK(length(placement_token)=32),
    store_id TEXT NOT NULL CHECK(length(store_id)>=1),
    restore_generation INTEGER NOT NULL CHECK(restore_generation>=1),
    generation INTEGER NOT NULL CHECK(generation>=1),
    baseline_position INTEGER NOT NULL CHECK(baseline_position>=0),
    scan_position INTEGER NOT NULL CHECK(scan_position>=baseline_position),
    created_by TEXT NOT NULL CHECK(length(created_by) BETWEEN 1 AND 2048),
    authority_ref TEXT NOT NULL CHECK(length(authority_ref) BETWEEN 1 AND 256),
    recipient_permission TEXT NOT NULL CHECK(recipient_permission IN ('control','manage')),
    recipient_access_generation BLOB NOT NULL CHECK(length(recipient_access_generation)=32),
    recipient_grant_revision INTEGER NOT NULL CHECK(recipient_grant_revision>=1),
    recipient_acl_revision INTEGER NOT NULL CHECK(recipient_acl_revision>=1),
    recipient_is_owner INTEGER NOT NULL CHECK(recipient_is_owner IN (0,1)),
    state TEXT NOT NULL CHECK(state IN ('active','lagged','disabled')),
    lag_reason TEXT CHECK(lag_reason IS NULL OR length(lag_reason) BETWEEN 1 AND 256),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(recipient_ref,subject),
    CHECK((state='lagged' AND lag_reason IS NOT NULL)
       OR (state<>'lagged' AND lag_reason IS NULL))
);

CREATE INDEX event_subscriptions_recipient_state
    ON event_subscriptions_current(recipient_ref,state,subject);
