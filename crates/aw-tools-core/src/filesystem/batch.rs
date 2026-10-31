//! Sealed DL-150 batch graph compiler. This module ONLY plans a fixed
//! dependency graph; it neither registers a model tool nor executes effects.
//! A host must supply its own original-journal subcall replay ledger before
//! mutation-capable execution may be exposed.
use super::{schema::Tool, status::Primary};
use serde_json::Value;

/// One small model schema for the future host-owned journaled executor.
/// Node schemas are intentionally NOT embedded in batch, preventing
/// permanent duplication of every native tool definition.
pub fn parameters() -> Value {
    let names = Tool::ALL.into_iter().map(Tool::name).collect::<Vec<_>>();
    serde_json::json!({"type":"object","properties":{
        "ops":{"type":"array","minItems":1,"maxItems":MAX_OPS,
            "items":{"type":"array","minItems":2,"maxItems":3,
                "prefixItems":[
                    {"type":"string","enum":names},
                    {"type":"object"},
                    {"type":"string","maxLength":MAX_CONDITION_BYTES}
                ]
            }
        }
    },"required":["ops"],"additionalProperties":false})
}
pub const MAX_OPS: usize = 64;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;
pub const MAX_CONDITION_BYTES: usize = 256;
const MAX_AST_NODES: usize = 128;
const MAX_AST_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidGraph,
    TooManyOps,
    TooLarge,
    InvalidNode,
    UnknownTool,
    InvalidArguments,
    InvalidCondition,
    ForwardReference,
    TooComplex,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Predicate {
    Complete,
    Ok,
    Unchanged,
    More,
    Incomplete,
    Fail,
    Mixed,
    Unknown,
    Skip,
    Terminal,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Root,
    Atom { step: usize, predicate: Predicate },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Status {
    pub primary: Primary,
    pub more: bool,
    pub incomplete: bool,
}
impl Status {
    fn matches(self, predicate: Predicate) -> bool {
        match predicate {
            Predicate::Complete => {
                matches!(self.primary, Primary::Ok | Primary::Unchanged)
                    && !self.more
                    && !self.incomplete
            }
            Predicate::Ok => self.primary == Primary::Ok,
            Predicate::Unchanged => self.primary == Primary::Unchanged,
            Predicate::More => self.more,
            Predicate::Incomplete => self.incomplete,
            Predicate::Fail => self.primary == Primary::Fail,
            Predicate::Mixed => self.primary == Primary::Mixed,
            Predicate::Unknown => self.primary == Primary::Unknown,
            Predicate::Skip => self.primary == Primary::Skip,
            Predicate::Terminal => true,
        }
    }
}
impl Expr {
    fn fingerprint(&self, hash: &mut sha2::Sha256) {
        use sha2::Digest as _;
        match self {
            Self::Root => hash.update([0]),
            Self::Atom { step, predicate } => {
                hash.update([1]);
                hash.update((*step as u64).to_le_bytes());
                hash.update([*predicate as u8]);
            }
            Self::And(a, b) => {
                hash.update([2]);
                a.fingerprint(hash);
                b.fingerprint(hash);
            }
            Self::Or(a, b) => {
                hash.update([3]);
                a.fingerprint(hash);
                b.fingerprint(hash);
            }
        }
    }
    fn references_ready(&self, states: &[Option<Status>]) -> bool {
        match self {
            Self::Root => true,
            Self::Atom { step, .. } => states.get(step - 1).is_some_and(Option::is_some),
            Self::And(a, b) | Self::Or(a, b) => {
                a.references_ready(states) && b.references_ready(states)
            }
        }
    }
    fn test(&self, states: &[Option<Status>]) -> bool {
        match self {
            Self::Root => true,
            Self::Atom { step, predicate } => states[step - 1]
                .expect("all referenced states must be settled")
                .matches(*predicate),
            Self::And(a, b) => a.test(states) && b.test(states),
            Self::Or(a, b) => a.test(states) || b.test(states),
        }
    }
    /// None means the referenced terminal outcomes are not all known yet.
    /// A ready false expression becomes skip condition, with no effects.
    pub fn evaluate(&self, states: &[Option<Status>]) -> Option<bool> {
        self.references_ready(states).then(|| self.test(states))
    }
}
#[derive(Debug, Clone)]
pub struct Step {
    pub tool: Tool,
    pub arguments: Value,
    pub when: Expr,
}
#[derive(Debug, Clone)]
pub struct Graph {
    steps: Vec<Step>,
}
impl Graph {
    /// Stable semantic identity, including every argument and dependency.
    /// This is a replay-domain fingerprint, NOT a source/grant capability.
    pub fn fingerprint(&self) -> [u8; 32] {
        use sha2::{Digest as _, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-core-graph-v1");
        hash.update((self.steps.len() as u64).to_le_bytes());
        for step in &self.steps {
            let name = step.tool.name();
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            let args = serde_json::to_vec(&step.arguments).expect("validated, finite JSON");
            hash.update((args.len() as u64).to_le_bytes());
            hash.update(&args);
            step.when.fingerprint(&mut hash);
        }
        hash.finalize().into()
    }
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }
    /// Validates the sealed tuple representation and delegates nested
    /// arguments to the original registered tool schema/authorization host.
    pub fn compile(
        input: &Value,
        mut validate: impl FnMut(Tool, &Value) -> bool,
    ) -> Result<Self, Error> {
        if serde_json::to_vec(input)
            .map_err(|_| Error::InvalidGraph)?
            .len()
            > MAX_BATCH_BYTES
        {
            return Err(Error::TooLarge);
        }
        let object = input.as_object().ok_or(Error::InvalidGraph)?;
        if object.len() != 1 {
            return Err(Error::InvalidGraph);
        }
        let ops = object
            .get("ops")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidGraph)?;
        if ops.is_empty() || ops.len() > MAX_OPS {
            return Err(Error::TooManyOps);
        }
        let mut result = Vec::with_capacity(ops.len());
        for (idx, raw) in ops.iter().enumerate() {
            let tuple = raw.as_array().ok_or(Error::InvalidNode)?;
            if !(2..=3).contains(&tuple.len()) {
                return Err(Error::InvalidNode);
            }
            let name = tuple[0].as_str().ok_or(Error::InvalidNode)?;
            let tool = Tool::ALL
                .into_iter()
                .find(|tool| tool.name() == name)
                .ok_or(Error::UnknownTool)?;
            let arguments = &tuple[1];
            if !arguments.is_object() || !validate(tool, arguments) {
                return Err(Error::InvalidArguments);
            }
            let when = if tuple.len() == 3 {
                let source = tuple[2].as_str().ok_or(Error::InvalidCondition)?;
                if source.len() > MAX_CONDITION_BYTES || source.is_empty() {
                    return Err(Error::InvalidCondition);
                }
                Parser::new(source, idx + 1).compile()?
            } else if idx == 0 {
                Expr::Root
            } else {
                Expr::Atom {
                    step: idx,
                    predicate: Predicate::Complete,
                }
            };
            result.push(Step {
                tool,
                arguments: arguments.clone(),
                when,
            });
        }
        Ok(Self { steps: result })
    }
}
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    current: usize,
    nodes: usize,
}
impl<'a> Parser<'a> {
    fn new(source: &'a str, current: usize) -> Self {
        Self {
            bytes: source.as_bytes(),
            pos: 0,
            current,
            nodes: 0,
        }
    }
    fn consume_space(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn consume(&mut self, byte: u8) -> bool {
        self.consume_space();
        if self.bytes.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn node(&mut self) -> Result<(), Error> {
        self.nodes += 1;
        if self.nodes > MAX_AST_NODES {
            return Err(Error::TooComplex);
        }
        Ok(())
    }
    fn compile(mut self) -> Result<Expr, Error> {
        if self.bytes == b"*" {
            return Ok(Expr::Root);
        }
        let result = self.parse_or(0)?;
        self.consume_space();
        if self.pos != self.bytes.len() {
            return Err(Error::InvalidCondition);
        }
        Ok(result)
    }
    fn parse_or(&mut self, depth: usize) -> Result<Expr, Error> {
        let mut left = self.parse_and(depth + 1)?;
        while self.consume(b'|') {
            let right = self.parse_and(depth + 1)?;
            self.node()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn parse_and(&mut self, depth: usize) -> Result<Expr, Error> {
        let mut left = self.parse_atom(depth + 1)?;
        while self.consume(b'&') {
            let right = self.parse_atom(depth + 1)?;
            self.node()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn parse_atom(&mut self, depth: usize) -> Result<Expr, Error> {
        if depth > MAX_AST_DEPTH {
            return Err(Error::TooComplex);
        }
        if self.consume(b'(') {
            let nested = self.parse_or(depth + 1)?;
            if !self.consume(b')') {
                return Err(Error::InvalidCondition);
            }
            return Ok(nested);
        }
        self.consume_space();
        let start = self.pos;
        while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(Error::InvalidCondition);
        }
        if self.bytes[start] == b'0' {
            return Err(Error::InvalidCondition);
        }
        let coordinate = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| Error::InvalidCondition)?
            .parse::<usize>()
            .map_err(|_| Error::TooComplex)?;
        if coordinate == 0 || coordinate >= self.current {
            return Err(Error::ForwardReference);
        }
        let predicate = if self.consume(b'.') {
            let begin = self.pos;
            if self.bytes.get(self.pos) == Some(&b'*') {
                self.pos += 1;
            } else {
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|b| b.is_ascii_lowercase())
                {
                    self.pos += 1;
                }
            }
            match &self.bytes[begin..self.pos] {
                b"ok" => Predicate::Ok,
                b"unchanged" => Predicate::Unchanged,
                b"more" => Predicate::More,
                b"incomplete" => Predicate::Incomplete,
                b"fail" => Predicate::Fail,
                b"mixed" => Predicate::Mixed,
                b"unknown" => Predicate::Unknown,
                b"skip" => Predicate::Skip,
                b"*" => Predicate::Terminal,
                _ => return Err(Error::InvalidCondition),
            }
        } else {
            Predicate::Complete
        };
        self.node()?;
        Ok(Expr::Atom {
            step: coordinate,
            predicate,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn status(primary: Primary, more: bool, incomplete: bool) -> Option<Status> {
        Some(Status {
            primary,
            more,
            incomplete,
        })
    }
    #[test]
    fn single_graph_schema_does_not_duplicate_nested_tool_schemas() {
        let params = parameters();
        assert!(params.to_string().len() < 1024);
        assert_eq!(params["properties"]["ops"]["items"]["minItems"], 2);
        assert_eq!(params["properties"]["ops"]["items"]["maxItems"], 3);
        let names = params["properties"]["ops"]["items"]["prefixItems"][0]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(names.len(), Tool::ALL.len());
        assert!(!names.iter().any(|name| name == "batch" || name == "bash"));
    }
    #[test]
    fn immutable_graph_hash_binds_arguments_and_conditions() {
        let construct = |name, cond| {
            Graph::compile(
                &serde_json::json!({"ops":[
                    ["read",{"path":name}],
                    ["write",{"path":"x","content":"y"},cond]
                ]}),
                |_, _| true,
            )
            .unwrap()
        };
        let g = construct("a", "1");
        assert_eq!(g.fingerprint(), construct("a", "1").fingerprint());
        assert_ne!(g.fingerprint(), construct("a", "1.*").fingerprint());
        assert_ne!(g.fingerprint(), construct("b", "1").fingerprint());
    }
    #[test]
    fn graph_compiles_default_sequence_parallel_roots_and_joins() {
        let graph = Graph::compile(
            &json!({"ops":[
                ["read",{"path":"a"}],
                ["read",{"path":"b"},"*"],
                ["edit",{"path":"a","edits":[["x","y"]]},"1&2"],
                ["rm",{"paths":["tmp"]}],
                ["read",{"path":"fallback"},"3.fail"]
            ]}),
            |_, _| true,
        )
        .unwrap();
        assert_eq!(graph.steps().len(), 5);
        let unknown = vec![None; 5];
        assert_eq!(graph.steps()[0].when.evaluate(&unknown), Some(true));
        assert_eq!(graph.steps()[1].when.evaluate(&unknown), Some(true));
        assert_eq!(graph.steps()[2].when.evaluate(&unknown), None);
        let mut done = unknown.clone();
        done[0] = status(Primary::Ok, false, false);
        done[1] = status(Primary::Unchanged, false, false);
        assert_eq!(graph.steps()[2].when.evaluate(&done), Some(true));
        done[2] = status(Primary::Fail, false, false);
        assert_eq!(graph.steps()[3].when.evaluate(&done), Some(false));
        assert_eq!(graph.steps()[4].when.evaluate(&done), Some(true));
    }
    #[test]
    fn paging_not_complete_but_predicates_are_orthogonal() {
        let expr = Parser::new("1&2", 3).compile().unwrap();
        let states = [
            status(Primary::Ok, true, false),
            status(Primary::Ok, false, false),
        ];
        assert_eq!(expr.evaluate(&states), Some(false));
        let expr = Parser::new("1.more|2.incomplete", 3).compile().unwrap();
        assert_eq!(expr.evaluate(&states), Some(true));
        let unresolved = [states[0], None];
        assert_eq!(
            expr.evaluate(&unresolved),
            None,
            "even OR must await all referenced terminal states"
        );
        let branches = Parser::new("1.fail&(2.*|2.skip)", 3).compile().unwrap();
        let states = [
            status(Primary::Fail, false, false),
            status(Primary::Skip, false, false),
        ];
        assert_eq!(branches.evaluate(&states), Some(true));
    }
    #[test]
    fn known_error_branch_excludes_ambiguous_side_effects() {
        let uncertain = [status(Primary::Unknown, false, false)];
        assert_eq!(
            Parser::new("1.fail", 2)
                .compile()
                .unwrap()
                .evaluate(&uncertain),
            Some(false)
        );
        assert_eq!(
            Parser::new("1.unknown", 2)
                .compile()
                .unwrap()
                .evaluate(&uncertain),
            Some(true)
        );
        assert_eq!(
            Parser::new("1.*", 2)
                .compile()
                .unwrap()
                .evaluate(&uncertain),
            Some(true)
        );
        let failed = [status(Primary::Fail, false, false)];
        assert_eq!(
            Parser::new("1.fail", 2)
                .compile()
                .unwrap()
                .evaluate(&failed),
            Some(true)
        );
    }
    #[test]
    fn rejects_forward_nested_expressions_and_unregistered_tools() {
        for source in [
            "2",
            "0",
            "01",
            "1.foo",
            "1&&1",
            "1|",
            "(1",
            "1)",
            "1+2",
            "1.output",
            "1.err",
            "1.done",
            "1.partial",
            "1.uncertain",
        ] {
            assert!(Parser::new(source, 2).compile().is_err(), "{source}");
        }
        assert!(matches!(
            Graph::compile(
                &json!({"ops":[
                    ["batch",{}]
                ]}),
                |_, _| true
            ),
            Err(Error::UnknownTool)
        ));
        assert!(matches!(
            Graph::compile(
                &json!({"ops":[
                    ["read",{"path":"a"}],["edit",{"path":"b"},"1"]
                ]}),
                |tool, _| tool != Tool::Edit
            ),
            Err(Error::InvalidArguments)
        ));
        assert!(matches!(
            Graph::compile(
                &json!({"ops":[
                    ["read",{}],["read",{},"3"]
                ]}),
                |_, _| true
            ),
            Err(Error::ForwardReference)
        ));
    }
}
