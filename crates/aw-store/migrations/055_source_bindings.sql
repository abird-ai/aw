-- Named source state belongs to the logical Sandbox, not the registry.
-- Existing Sandboxes have an empty SourceSet; no inferred source identity.
CREATE TABLE sandbox_source_bindings_current(
    sandbox_id TEXT NOT NULL REFERENCES sandboxes_current(sandbox_id) ON DELETE RESTRICT,
    source_name TEXT NOT NULL CHECK(length(source_name) BETWEEN 1 AND 64
        AND source_name GLOB '[a-z]*' AND source_name NOT GLOB '*[^a-z0-9_-]*'),
    driver_key TEXT NOT NULL CHECK(length(CAST(driver_key AS BLOB)) BETWEEN 1 AND 256),
    binding_schema INTEGER NOT NULL CHECK(binding_schema BETWEEN 1 AND 4294967295),
    source_ref TEXT NOT NULL CHECK(length(CAST(source_ref AS BLOB)) BETWEEN 1 AND 2048),
    materialization TEXT NOT NULL CHECK(length(CAST(materialization AS BLOB)) BETWEEN 1 AND 2048),
    access TEXT NOT NULL CHECK(access IN ('ro','rw')),
    checkpoint_policy TEXT NOT NULL CHECK(checkpoint_policy IN ('required','ephemeral')),
    binding_rev INTEGER NOT NULL CHECK(binding_rev>=1),
    compatibility TEXT NOT NULL CHECK(compatibility IN ('compatible','recovery_required')),
    PRIMARY KEY(sandbox_id,source_name)
) WITHOUT ROWID;
