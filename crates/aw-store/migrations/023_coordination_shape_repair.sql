-- Unnumbered post-Phase-39 compatibility repair.
-- Some stores reached schema v22 using an older Phase-35 migration shape that
-- did not yet include every per-process ownership column. The Rust migration
-- conditionally adds only missing columns before this metadata step.

CREATE INDEX IF NOT EXISTS runs_by_local_process
    ON runs(owner_process_id,owner_store_generation,status,attach_status);
CREATE INDEX IF NOT EXISTS context_operations_by_local_process
    ON context_operations(owner_process_id,owner_store_generation,state);
CREATE INDEX IF NOT EXISTS web_requests_by_local_process
    ON web_requests(owner_process_id,owner_store_generation,state);

UPDATE store_meta SET value='23' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=23,activation_epoch=23,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=23,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=23;
