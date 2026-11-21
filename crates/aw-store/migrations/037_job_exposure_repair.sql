-- Repair v36 Agent exposure coverage after all authoritative branch creation paths
-- converge on the shared branch.created publication helper. No new semantic owner.

UPDATE store_meta SET value='37' WHERE key='schema_epoch';
UPDATE schema_meta
SET schema_epoch=37,activation_epoch=37,migration_phase='ready',updated_at=0
WHERE singleton=1;
UPDATE migration_lease
SET holder_client_id=NULL,generation=generation+1,current_epoch=37,target_epoch=NULL,
    lease_expires_at=NULL,updated_at=0
WHERE singleton=1;
PRAGMA user_version=37;
