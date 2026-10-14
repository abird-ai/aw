-- Phase 35: direct-mode run ownership and recovery fencing.
-- Only the owner-lock process may migrate to this schema. Trusted-local direct
-- clients require this exact current schema and never run migrations.

ALTER TABLE runs ADD COLUMN owner_process_id TEXT
    REFERENCES local_process_incarnations(process_id);
ALTER TABLE runs ADD COLUMN owner_store_generation INTEGER
    CHECK(owner_store_generation IS NULL OR owner_store_generation>0);

CREATE INDEX runs_by_local_process
    ON runs(owner_process_id,owner_store_generation,status,attach_status);

INSERT INTO store_meta(key,value) VALUES('coordination_schema','2')
ON CONFLICT(key) DO UPDATE SET value=excluded.value;
UPDATE store_meta SET value='20' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=20,activation_epoch=20,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=20,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;


ALTER TABLE context_operations ADD COLUMN owner_process_id TEXT
    REFERENCES local_process_incarnations(process_id);
ALTER TABLE context_operations ADD COLUMN owner_store_generation INTEGER
    CHECK(owner_store_generation IS NULL OR owner_store_generation>0);
CREATE INDEX context_operations_by_local_process
    ON context_operations(owner_process_id,owner_store_generation,state);

ALTER TABLE web_requests ADD COLUMN owner_process_id TEXT
    REFERENCES local_process_incarnations(process_id);
ALTER TABLE web_requests ADD COLUMN owner_store_generation INTEGER
    CHECK(owner_store_generation IS NULL OR owner_store_generation>0);
CREATE INDEX web_requests_by_local_process
    ON web_requests(owner_process_id,owner_store_generation,state);

PRAGMA user_version=20;
