//! Host-neutral DL-150 conformance vectors for temporary differential consumers.
//!
//! This module is data + pure shared semantics only. It does not execute tools,
//! open files, grant authority, launch processes, or depend on AW/dotlink host
//! types. Consumers should enable the `conformance` feature only in tests.
use crate::filesystem::{
    batch::{Graph, NodeTool, Status},
    schema::{self, SchemaCaps, Tool},
    status::{self, Primary},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Increment only for an intentional incompatible vector-format change.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestVector {
    pub id: String,
    pub tool: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaVector {
    pub tool: String,
    pub action: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphVector {
    pub id: String,
    pub graph: Value,
    pub accepted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateVector {
    pub components: Vec<Primary>,
    pub expected: Primary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerVector {
    pub id: String,
    pub rendered: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fixture {
    pub version: u32,
    pub schemas: Vec<SchemaVector>,
    pub requests: Vec<RequestVector>,
    pub graphs: Vec<GraphVector>,
    pub aggregates: Vec<AggregateVector>,
    pub markers: Vec<MarkerVector>,
}

fn action_label(tool: Tool) -> &'static str {
    use crate::filesystem::access::Action;
    match tool.action() {
        Action::Read => "read",
        Action::Write => "write",
        Action::Edit => "edit",
        Action::Copy => "copy",
        Action::Move => "move",
        Action::Remove => "remove",
        Action::Shell => "shell",
    }
}

/// Exact canonical schema projection for differential comparison. Host-only
/// schema additions (for example AW `parallel`) are controlled by `caps`.
pub fn schemas(caps: SchemaCaps) -> Vec<SchemaVector> {
    Tool::ALL
        .into_iter()
        .map(|tool| {
            let spec = schema::spec(tool, caps);
            SchemaVector {
                tool: spec.name.to_owned(),
                action: action_label(tool).to_owned(),
                parameters: spec.parameters,
            }
        })
        .collect()
}

/// One syntactically canonical request per shared filesystem tool. These are
/// intentionally transport-free and do not encode authority or filesystem
/// setup; a differential host supplies identical prepared fixtures to both
/// implementations before executing them.
pub fn requests() -> Vec<RequestVector> {
    [
        ("ls-basic", "ls", json!({"path":".","limit":16})),
        ("find-glob", "find", json!({"path":".","glob":"**/*.rs","limit":16})),
        ("grep-content", "grep", json!({"path":".","pattern":"needle","mode":"content","limit":8})),
        ("read-cursor", "read", json!({"path":"a.txt","line":1,"lines":4,"byte":0,"numbered":false})),
        ("write-text", "write", json!({"path":"new.txt","content":"hello\n"})),
        ("edit-snapshot", "edit", json!({"path":"a.txt","edits":[["old","new",false]]})),
        ("patch-update", "patch", json!({"patch":"*** Begin Patch\n*** Update File: a.txt\n@@\n-old\n+new\n*** End Patch"})),
        ("copy-exact", "cp", json!({"from":"a.txt","to":"copy.txt"})),
        ("move-exact", "mv", json!({"from":"copy.txt","to":"moved.txt"})),
        ("remove-exact", "rm", json!({"paths":["moved.txt"],"recursive":false})),
        ("bread-window", "bread", json!({"path":"blob.bin","offset":0,"limit":16,"format":"hex"})),
        ("bwrite-hex", "bwrite", json!({"path":"new.bin","data":"000102ff","encoding":"hex"})),
        ("bedit-range", "bedit", json!({"path":"blob.bin","edits":[{"offset":1,"length":1,"data":"aa","encoding":"hex"}]})),
    ]
    .into_iter()
    .map(|(id, tool, arguments)| RequestVector {
        id: id.to_owned(),
        tool: tool.to_owned(),
        arguments,
    })
    .collect()
}

/// Pure Graph acceptance vectors. Consumers resolve only the node names present
/// in the vector, matching the host-resolved compiler contract.
pub fn graphs() -> Vec<GraphVector> {
    vec![
        GraphVector {
            id: "native-default-dependency".into(),
            graph: json!({"ops":[
                ["read",{"path":"a.txt"}],
                ["grep",{"path":".","pattern":"x"}]
            ]}),
            accepted: true,
        },
        GraphVector {
            id: "mixed-shell-explicit-condition".into(),
            graph: json!({"ops":[
                ["read",{"path":"a.txt"}],
                ["bash",{"command":"true"},"1.ok | 1.unchanged"]
            ]}),
            accepted: true,
        },
        GraphVector {
            id: "forward-reference-rejected".into(),
            graph: json!({"ops":[
                ["read",{"path":"a.txt"},"2.ok"],
                ["read",{"path":"b.txt"}]
            ]}),
            accepted: false,
        },
        GraphVector {
            id: "unknown-node-rejected".into(),
            graph: json!({"ops":[["not-a-tool",{}]]}),
            accepted: false,
        },
        GraphVector {
            id: "bread-direct-only-rejected".into(),
            graph: json!({"ops":[["bread",{"path":"blob.bin","format":"hex"}]]}),
            accepted: false,
        },
        GraphVector {
            id: "invalid-predicate-rejected".into(),
            graph: json!({"ops":[
                ["read",{"path":"a.txt"}],
                ["read",{"path":"b.txt"},"1.err"]
            ]}),
            accepted: false,
        },
    ]
}

pub fn aggregates() -> Vec<AggregateVector> {
    use Primary as P;
    vec![
        AggregateVector {
            components: vec![P::Skip],
            expected: P::Skip,
        },
        AggregateVector {
            components: vec![P::Unchanged, P::Skip],
            expected: P::Unchanged,
        },
        AggregateVector {
            components: vec![P::Ok, P::Unchanged],
            expected: P::Ok,
        },
        AggregateVector {
            components: vec![P::Fail, P::Fail],
            expected: P::Fail,
        },
        AggregateVector {
            components: vec![P::Unknown, P::Unknown],
            expected: P::Unknown,
        },
        AggregateVector {
            components: vec![P::Fail, P::Unknown],
            expected: P::Mixed,
        },
        AggregateVector {
            components: vec![P::Unknown, P::Ok],
            expected: P::Mixed,
        },
        AggregateVector {
            components: vec![P::Mixed, P::Skip],
            expected: P::Mixed,
        },
    ]
}

pub fn markers() -> Vec<MarkerVector> {
    vec![
        MarkerVector {
            id: "query-more".into(),
            rendered: status::query(Some(17), None),
        },
        MarkerVector {
            id: "query-incomplete".into(),
            rendered: status::query(None, Some("walk,gitignore")),
        },
        MarkerVector {
            id: "query-both".into(),
            rendered: status::query(Some(9), Some("budget")),
        },
        MarkerVector {
            id: "read-more".into(),
            rendered: status::read(Some((42, 7)), false),
        },
        MarkerVector {
            id: "read-io-prefix".into(),
            rendered: status::read(Some((42, 7)), true),
        },
    ]
}

pub fn fixture_with_caps(caps: SchemaCaps) -> Fixture {
    Fixture {
        version: FORMAT_VERSION,
        schemas: schemas(caps),
        requests: requests(),
        graphs: graphs(),
        aggregates: aggregates(),
        markers: markers(),
    }
}

pub fn fixture() -> Fixture {
    fixture_with_caps(SchemaCaps::default())
}

/// Serialize a complete fixture for an external differential harness whose
/// host-specific schema ceilings are known.
pub fn fixture_json_with_caps(caps: SchemaCaps) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&fixture_with_caps(caps))
}

/// Serialize the default-capability fixture for external differential harnesses
/// that cannot yet depend on the pinned shared crate directly.
pub fn fixture_json() -> Result<String, serde_json::Error> {
    fixture_json_with_caps(SchemaCaps::default())
}

/// Validate the fixture against this exact shared implementation revision.
/// Differential consumers should run this first so a stale copied expectation
/// cannot be mistaken for a product mismatch.
pub fn validate() -> Result<(), String> {
    for vector in graphs() {
        let result = Graph::compile_with(
            &vector.graph,
            |name| match name {
                "bash" => Some(NodeTool::Bash),
                other => Tool::GRAPH_NATIVE
                    .into_iter()
                    .find(|tool| tool.name() == other)
                    .map(NodeTool::Native),
            },
            |_, arguments| arguments.is_object(),
        );
        if result.is_ok() != vector.accepted {
            return Err(format!("graph vector {} acceptance changed", vector.id));
        }
    }
    for vector in aggregates() {
        if status::aggregate(vector.components) != vector.expected {
            return Err("aggregate vector changed".into());
        }
    }
    // Exercise condition evaluation, not only parser acceptance: the shell node
    // in the accepted mixed vector must run for either success-family outcome.
    let mixed = graphs()
        .into_iter()
        .find(|vector| vector.id == "mixed-shell-explicit-condition")
        .ok_or_else(|| "mixed graph vector missing".to_owned())?;
    let graph = Graph::compile_with(
        &mixed.graph,
        |name| match name {
            "bash" => Some(NodeTool::Bash),
            other => Tool::GRAPH_NATIVE
                .into_iter()
                .find(|tool| tool.name() == other)
                .map(NodeTool::Native),
        },
        |_, arguments| arguments.is_object(),
    )
    .map_err(|_| "mixed graph no longer compiles".to_owned())?;
    let condition = &graph.steps()[1].when;
    for primary in [Primary::Ok, Primary::Unchanged] {
        if condition.evaluate(&[Some(Status {
            primary,
            more: false,
            incomplete: false,
        })]) != Some(true)
        {
            return Err("mixed graph success condition changed".into());
        }
    }
    if condition.evaluate(&[Some(Status {
        primary: Primary::Fail,
        more: false,
        incomplete: false,
    })]) != Some(false)
    {
        return Err("mixed graph fail condition changed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_fixture_is_self_consistent_and_complete() {
        validate().unwrap();
        assert_eq!(requests().len(), Tool::ALL.len());
        assert_eq!(schemas(SchemaCaps::default()).len(), Tool::ALL.len());
        let names = schemas(SchemaCaps::default())
            .into_iter()
            .map(|schema| schema.tool)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), Tool::ALL.len());
    }

    #[test]
    fn fixture_json_round_trip_is_stable_shape() {
        let fixture = fixture();
        let encoded = serde_json::to_vec(&fixture).unwrap();
        let decoded: Fixture = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, fixture);
        assert_eq!(decoded.version, FORMAT_VERSION);
    }
}
