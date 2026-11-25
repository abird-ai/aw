-- Extend the original immutable receipt/audit owner to Project-scoped
-- construction without a synthetic Session. Preserve all existing positions.
ALTER TABLE audit RENAME TO audit_v59;
CREATE TABLE audit (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT REFERENCES sessions(id),
    project_ref TEXT,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    data_json TEXT NOT NULL CHECK(json_valid(data_json)),
    created_at INTEGER NOT NULL,
    CHECK((session_id IS NOT NULL AND project_ref IS NULL)
       OR (session_id IS NULL AND project_ref IS NOT NULL
           AND action='construction.preparation'))
);
INSERT INTO audit(seq,session_id,actor,action,data_json,created_at)
SELECT seq,session_id,actor,action,data_json,created_at FROM audit_v59 ORDER BY seq;
DROP TABLE audit_v59;
CREATE INDEX audit_by_session ON audit(session_id,seq);
CREATE TRIGGER audit_immutable_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT,'audit is immutable'); END;
CREATE TRIGGER audit_immutable_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT,'audit is immutable'); END;
CREATE INDEX construction_receipt_replay ON audit(
    COALESCE(json_extract(data_json,'$.scope'),session_id||'/'||json_extract(data_json,'$.parent_branch_id')),
    COALESCE(json_extract(data_json,'$.operation'),'agent_create'),
    json_extract(data_json,'$.request_id'),
    seq DESC
) WHERE action IN ('agent.create.preparation','construction.preparation');
CREATE UNIQUE INDEX construction_source_prepare_id ON audit(
    json_extract(data_json,'$.sources.prepare_id')
) WHERE action='construction.preparation'
    AND json_extract(data_json,'$.state')='prepared'
    AND json_type(data_json,'$.sources')='object';
