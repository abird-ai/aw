-- A bound managed Sandbox stays source-only even after its final release.
-- The immutable release fact is the authority boundary, not another owner.
-- This epoch also prevents an older writer from ignoring that fact.
CREATE INDEX IF NOT EXISTS sandbox_held_source_boundary ON commit_log(entity_id)
WHERE entity_type='sandbox' AND kind='sandbox.sources.changed'
  AND json_type(payload_json,'$.held_source_boundary')='true';
