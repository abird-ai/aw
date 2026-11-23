-- DL-200 Phase 2: source-local Event revision owner.
-- Delivery ownership remains legacy until delivery_cutover_rev is set later.

CREATE TABLE event_sources_current(
    source_ref TEXT PRIMARY KEY CHECK(length(source_ref) BETWEEN 1 AND 2048),
    next_event_rev INTEGER NOT NULL CHECK(next_event_rev>=1),
    delivery_cutover_rev INTEGER
        CHECK(delivery_cutover_rev IS NULL OR delivery_cutover_rev>=1),
    updated_at INTEGER NOT NULL,
    CHECK(delivery_cutover_rev IS NULL OR delivery_cutover_rev<=next_event_rev)
);
