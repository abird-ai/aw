-- The existing immutable construction audit receipt is the replay owner.
-- Index its exact key; legacy task-only receipts keep their original scope.
CREATE INDEX construction_receipt_replay ON audit(
    COALESCE(json_extract(data_json,'$.scope'),session_id||'/'||json_extract(data_json,'$.parent_branch_id')),
    COALESCE(json_extract(data_json,'$.operation'),'agent_create'),
    json_extract(data_json,'$.request_id'),
    seq DESC
) WHERE action IN ('agent.create.preparation','construction.preparation');

-- One operation-owned preparation ID cannot be claimed by another request.
-- Later Unknown/Applied/Rejected facts preserve the original claim.
CREATE UNIQUE INDEX construction_source_prepare_id ON audit(
    json_extract(data_json,'$.sources.prepare_id')
) WHERE action='construction.preparation'
    AND json_extract(data_json,'$.state')='prepared'
    AND json_type(data_json,'$.sources')='object';

-- Legacy receipt payload and timestamp identify its original security audit
-- user, not a later mapping of the raw principal.
CREATE INDEX construction_legacy_actor ON security_audit_events(resource_id,created_at)
WHERE action='agent.create.preparation' AND resource_type='session';
