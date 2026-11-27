-- Immutable links to the existing disclosure DAG. Plans capture the
-- original read ceiling; artifacts reference the plan, committed nodes
-- retain the original manifest through history ancestry.
CREATE TABLE context_plan_disclosure (
    plan_id TEXT PRIMARY KEY REFERENCES context_plans(id),
    manifest_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest)
);
CREATE TABLE node_context_disclosure (
    node_id TEXT PRIMARY KEY REFERENCES nodes(id),
    manifest_digest TEXT NOT NULL REFERENCES disclosure_manifest_nodes(node_digest)
);
CREATE TRIGGER context_plan_disclosure_immutable_update BEFORE UPDATE ON context_plan_disclosure BEGIN
    SELECT RAISE(ABORT,'context disclosure is immutable');
END;
CREATE TRIGGER context_plan_disclosure_immutable_delete BEFORE DELETE ON context_plan_disclosure BEGIN
    SELECT RAISE(ABORT,'context disclosure is immutable');
END;
CREATE TRIGGER node_context_disclosure_immutable_update BEFORE UPDATE ON node_context_disclosure BEGIN
    SELECT RAISE(ABORT,'context disclosure is immutable');
END;
CREATE TRIGGER node_context_disclosure_immutable_delete BEFORE DELETE ON node_context_disclosure BEGIN
    SELECT RAISE(ABORT,'context disclosure is immutable');
END;
