//! Typed hierarchical execution references. Parsing and contextual canonicalization
//! are identity mechanics only; neither step grants authority or creates defaults.

use std::{
    fmt,
    num::{NonZeroU32, NonZeroU64},
    str::FromStr,
};

pub const MAX_HANDLE_BYTES: usize = 64;
pub const MAX_REF_BYTES: usize = 2048;
pub const MAX_REF_SEGMENTS: usize = 32;
const MAX_SUFFIX_SEGMENTS: usize = MAX_REF_SEGMENTS - 4;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handle(String);

impl Handle {
    pub fn new(value: impl Into<String>) -> Result<Self, RefError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.is_empty()
            || bytes.len() > MAX_HANDLE_BYTES
            || !bytes[0].is_ascii_alphanumeric()
            || !bytes[1..].iter().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'~' | b'-')
            })
        {
            return Err(RefError::InvalidHandle);
        }
        Ok(Self(value))
    }

    pub fn zero() -> Self {
        Self("0".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(NonZeroU64);

impl Revision {
    pub fn new(value: u64) -> Result<Self, RefError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(RefError::InvalidRevision)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub const fn nonzero(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeCoordinate(NonZeroU32);

impl NodeCoordinate {
    pub fn new(value: u32) -> Result<Self, RefError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(RefError::InvalidNodeCoordinate)
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for NodeCoordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.get().fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentRef {
    pub domain: Handle,
    pub identity: Handle,
    pub project: Handle,
    pub agent: Handle,
}

impl AgentRef {
    pub fn new(domain: Handle, identity: Handle, project: Handle, agent: Handle) -> Self {
        Self {
            domain,
            identity,
            project,
            agent,
        }
    }

    pub fn defaults() -> Self {
        Self {
            domain: Handle::zero(),
            identity: Handle::zero(),
            project: Handle::zero(),
            agent: Handle::zero(),
        }
    }

    pub fn canonical(&self) -> String {
        format!(
            "/d:{}/i:{}/p:{}/a:{}",
            self.domain, self.identity, self.project, self.agent
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExecutionSegment {
    Turn(Handle),
    Job(Handle),
    Node(NodeCoordinate),
}

impl ExecutionSegment {
    pub fn kind(&self) -> SegmentKind {
        match self {
            Self::Turn(_) => SegmentKind::Turn,
            Self::Job(_) => SegmentKind::Job,
            Self::Node(_) => SegmentKind::Node,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SegmentKind {
    Domain,
    Identity,
    Project,
    Agent,
    Turn,
    Job,
    Node,
}

impl SegmentKind {
    fn structural_index(self) -> Option<usize> {
        match self {
            Self::Domain => Some(0),
            Self::Identity => Some(1),
            Self::Project => Some(2),
            Self::Agent => Some(3),
            Self::Turn | Self::Job | Self::Node => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedExecutionRef {
    agent: AgentRef,
    suffix: Vec<ExecutionSegment>,
}

impl ResolvedExecutionRef {
    pub fn agent(agent: AgentRef) -> Self {
        Self {
            agent,
            suffix: Vec::new(),
        }
    }

    pub fn new(agent: AgentRef, suffix: Vec<ExecutionSegment>) -> Result<Self, RefError> {
        validate_suffix(&suffix)?;
        if suffix.len() > MAX_SUFFIX_SEGMENTS {
            return Err(RefError::TooManySegments);
        }
        let resolved = Self { agent, suffix };
        if resolved.canonical().len() > MAX_REF_BYTES {
            return Err(RefError::RefTooLong);
        }
        Ok(resolved)
    }

    pub fn agent_ref(&self) -> &AgentRef {
        &self.agent
    }

    pub fn suffix(&self) -> &[ExecutionSegment] {
        &self.suffix
    }

    pub fn canonical(&self) -> String {
        let mut out = self.agent.canonical();
        for segment in &self.suffix {
            match segment {
                ExecutionSegment::Turn(handle) => {
                    out.push_str("/t:");
                    out.push_str(handle.as_str());
                }
                ExecutionSegment::Job(handle) => {
                    out.push_str("/j:");
                    out.push_str(handle.as_str());
                }
                ExecutionSegment::Node(coordinate) => {
                    out.push_str("/n:");
                    out.push_str(&coordinate.to_string());
                }
            }
        }
        out
    }

    pub fn shortest_contextual_from(&self, current: &AgentRef) -> String {
        if self.agent == *current && self.suffix.is_empty() {
            return ".".into();
        }
        let mut out = if self.agent == *current {
            String::new()
        } else if self.agent.domain == current.domain
            && self.agent.identity == current.identity
            && self.agent.project == current.project
        {
            format!("a:{}", self.agent.agent)
        } else if self.agent.domain == current.domain && self.agent.identity == current.identity {
            format!("p:{}/a:{}", self.agent.project, self.agent.agent)
        } else if self.agent.domain == current.domain {
            format!(
                "i:{}/p:{}/a:{}",
                self.agent.identity, self.agent.project, self.agent.agent
            )
        } else {
            format!(
                "d:{}/i:{}/p:{}/a:{}",
                self.agent.domain, self.agent.identity, self.agent.project, self.agent.agent
            )
        };
        for segment in &self.suffix {
            if !out.is_empty() {
                out.push('/');
            }
            match segment {
                ExecutionSegment::Turn(handle) => {
                    out.push_str("t:");
                    out.push_str(handle.as_str());
                }
                ExecutionSegment::Job(handle) => {
                    out.push_str("j:");
                    out.push_str(handle.as_str());
                }
                ExecutionSegment::Node(coordinate) => {
                    out.push_str("n:");
                    out.push_str(&coordinate.to_string());
                }
            }
        }
        out
    }
}

impl fmt::Display for ResolvedExecutionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpecialRef {
    Current,
    Parent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionRef {
    raw: String,
    special: Option<SpecialRef>,
    absolute: bool,
    structural: [Option<Handle>; 4],
    suffix: Vec<ExecutionSegment>,
}

impl ExecutionRef {
    pub fn parse(input: &str) -> Result<Self, RefError> {
        if input == "." {
            return Ok(Self {
                raw: input.into(),
                special: Some(SpecialRef::Current),
                absolute: false,
                structural: std::array::from_fn(|_| None),
                suffix: Vec::new(),
            });
        }
        if input == "^" {
            return Ok(Self {
                raw: input.into(),
                special: Some(SpecialRef::Parent),
                absolute: false,
                structural: std::array::from_fn(|_| None),
                suffix: Vec::new(),
            });
        }
        if input.is_empty() || input == "/" || input.contains("//") {
            return Err(RefError::InvalidSyntax);
        }
        if input.len() > MAX_REF_BYTES {
            return Err(RefError::RefTooLong);
        }

        let absolute = input.starts_with('/');
        let body = if absolute { &input[1..] } else { input };
        if body.split('/').count() > MAX_REF_SEGMENTS {
            return Err(RefError::TooManySegments);
        }
        let mut structural: [Option<Handle>; 4] = std::array::from_fn(|_| None);
        let mut suffix = Vec::new();
        let mut last_structural = None;
        let mut suffix_started = false;

        for raw_segment in body.split('/') {
            let (prefix, value) = raw_segment.split_once(':').ok_or(RefError::InvalidSyntax)?;
            if prefix.len() != 1 || value.is_empty() {
                return Err(RefError::InvalidSyntax);
            }
            let kind = match prefix.as_bytes()[0] {
                b'd' => SegmentKind::Domain,
                b'i' => SegmentKind::Identity,
                b'p' => SegmentKind::Project,
                b'a' => SegmentKind::Agent,
                b't' => SegmentKind::Turn,
                b'j' => SegmentKind::Job,
                b'n' => SegmentKind::Node,
                _ => return Err(RefError::InvalidSegment),
            };

            if let Some(index) = kind.structural_index() {
                if suffix_started
                    || last_structural.is_some_and(|last: usize| index <= last)
                    || structural[index].is_some()
                {
                    return Err(RefError::InvalidStructuralOrder);
                }
                structural[index] = Some(Handle::new(value)?);
                last_structural = Some(index);
            } else {
                suffix_started = true;
                let segment = match kind {
                    SegmentKind::Turn => ExecutionSegment::Turn(Handle::new(value)?),
                    SegmentKind::Job => ExecutionSegment::Job(Handle::new(value)?),
                    SegmentKind::Node => {
                        if value.len() > 1 && value.starts_with('0')
                            || !value.bytes().all(|byte| byte.is_ascii_digit())
                        {
                            return Err(RefError::InvalidNodeCoordinate);
                        }
                        let coordinate = value
                            .parse::<u32>()
                            .ok()
                            .and_then(NonZeroU32::new)
                            .map(NodeCoordinate)
                            .ok_or(RefError::InvalidNodeCoordinate)?;
                        ExecutionSegment::Node(coordinate)
                    }
                    _ => unreachable!(),
                };
                suffix.push(segment);
            }
        }

        if absolute {
            if structural.iter().any(Option::is_none) {
                return Err(RefError::PartialCanonical);
            }
        } else if structural.iter().all(Option::is_none) && suffix.is_empty() {
            return Err(RefError::InvalidSyntax);
        }

        validate_suffix(&suffix)?;
        if suffix.len() > MAX_SUFFIX_SEGMENTS {
            return Err(RefError::TooManySegments);
        }

        Ok(Self {
            raw: input.into(),
            special: None,
            absolute,
            structural,
            suffix,
        })
    }

    pub fn requested(&self) -> &str {
        &self.raw
    }

    pub const fn is_canonical(&self) -> bool {
        self.absolute
    }

    pub fn resolve(
        &self,
        current: Option<&AgentRef>,
        parent: Option<&AgentRef>,
    ) -> Result<ResolvedExecutionRef, RefError> {
        match self.special {
            Some(SpecialRef::Current) => {
                return current
                    .cloned()
                    .map(ResolvedExecutionRef::agent)
                    .ok_or(RefError::CurrentUnavailable)
            }
            Some(SpecialRef::Parent) => {
                return parent
                    .cloned()
                    .map(ResolvedExecutionRef::agent)
                    .ok_or(RefError::ParentUnavailable)
            }
            None => {}
        }

        let base = current.cloned().unwrap_or_else(AgentRef::defaults);
        let mut values = [base.domain, base.identity, base.project, base.agent];

        if self.absolute {
            for (index, value) in self.structural.iter().enumerate() {
                values[index] = value.clone().ok_or(RefError::PartialCanonical)?;
            }
        } else if let Some(first) = self.structural.iter().position(Option::is_some) {
            for (index, value) in values.iter_mut().enumerate().skip(first) {
                *value = self.structural[index].clone().unwrap_or_else(Handle::zero);
            }
        }

        ResolvedExecutionRef::new(
            AgentRef::new(
                values[0].clone(),
                values[1].clone(),
                values[2].clone(),
                values[3].clone(),
            ),
            self.suffix.clone(),
        )
    }
}

impl FromStr for ExecutionRef {
    type Err = RefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

fn validate_suffix(suffix: &[ExecutionSegment]) -> Result<(), RefError> {
    for (index, segment) in suffix.iter().enumerate() {
        if index > 0 && matches!(segment, ExecutionSegment::Turn(_)) {
            return Err(RefError::InvalidStaticTransition);
        }
        if matches!(segment, ExecutionSegment::Node(_))
            && (index == 0 || !matches!(suffix[index - 1], ExecutionSegment::Job(_)))
        {
            return Err(RefError::InvalidStaticTransition);
        }
        if index > 0
            && matches!(suffix[index - 1], ExecutionSegment::Turn(_))
            && !matches!(segment, ExecutionSegment::Job(_))
        {
            return Err(RefError::InvalidStaticTransition);
        }
        if index > 0
            && matches!(suffix[index - 1], ExecutionSegment::Node(_))
            && !matches!(segment, ExecutionSegment::Job(_))
        {
            return Err(RefError::InvalidStaticTransition);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefError {
    InvalidSyntax,
    InvalidSegment,
    InvalidHandle,
    InvalidStructuralOrder,
    PartialCanonical,
    InvalidNodeCoordinate,
    InvalidStaticTransition,
    RefTooLong,
    TooManySegments,
    InvalidRevision,
    CurrentUnavailable,
    ParentUnavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(value: &str) -> Handle {
        Handle::new(value).unwrap()
    }

    fn current() -> AgentRef {
        AgentRef::new(h("abird"), h("pvl"), h("zg"), h("7"))
    }

    #[test]
    fn handle_grammar_is_ascii_canonical_and_bounded() {
        for valid in ["0", "a", "A-1", "abc.def_ghi~jkl"] {
            assert_eq!(Handle::new(valid).unwrap().as_str(), valid);
        }
        for invalid in ["", "-a", "a/b", "a:b", "é", "a%20b"] {
            assert_eq!(Handle::new(invalid), Err(RefError::InvalidHandle));
        }
        assert!(Handle::new("a".repeat(MAX_HANDLE_BYTES)).is_ok());
        assert!(Handle::new("a".repeat(MAX_HANDLE_BYTES + 1)).is_err());
    }

    #[test]
    fn canonical_refs_require_full_structural_tuple_and_preserve_defaults() {
        let parsed = ExecutionRef::parse("/d:abird/i:0/p:zg/a:0/t:5/j:x/n:4").unwrap();
        assert!(parsed.is_canonical());
        assert_eq!(
            parsed.resolve(None, None).unwrap().canonical(),
            "/d:abird/i:0/p:zg/a:0/t:5/j:x/n:4"
        );
        for invalid in ["/a:0", "/d:abird/i:pvl", "/d:0/i:0/p:0/a:0/n:1"] {
            assert!(ExecutionRef::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn contextual_overlay_matches_normative_examples() {
        let c = current();
        let cases = [
            ("a:0", "/d:abird/i:pvl/p:zg/a:0"),
            ("a:2", "/d:abird/i:pvl/p:zg/a:2"),
            ("p:core", "/d:abird/i:pvl/p:core/a:0"),
            ("p:core/a:2", "/d:abird/i:pvl/p:core/a:2"),
            ("i:alice", "/d:abird/i:alice/p:0/a:0"),
            ("i:alice/a:2", "/d:abird/i:alice/p:0/a:2"),
            ("d:remote", "/d:remote/i:0/p:0/a:0"),
            (
                "d:remote/i:alice/p:core/a:2",
                "/d:remote/i:alice/p:core/a:2",
            ),
            ("t:5", "/d:abird/i:pvl/p:zg/a:7/t:5"),
            ("t:5/j:1", "/d:abird/i:pvl/p:zg/a:7/t:5/j:1"),
            ("a:2/t:3", "/d:abird/i:pvl/p:zg/a:2/t:3"),
            ("p:core/a:2/t:5/j:1", "/d:abird/i:pvl/p:core/a:2/t:5/j:1"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                ExecutionRef::parse(input)
                    .unwrap()
                    .resolve(Some(&c), None)
                    .unwrap()
                    .canonical(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn no_context_uses_fixed_structural_defaults_without_minting_authority() {
        assert_eq!(
            ExecutionRef::parse("p:core/a:2")
                .unwrap()
                .resolve(None, None)
                .unwrap()
                .canonical(),
            "/d:0/i:0/p:core/a:2"
        );
        assert!(matches!(
            ExecutionRef::parse(".").unwrap().resolve(None, None),
            Err(RefError::CurrentUnavailable)
        ));
    }

    #[test]
    fn current_parent_and_shortest_rendering_are_identity_only() {
        let c = current();
        let p = AgentRef::new(h("abird"), h("pvl"), h("zg"), h("0"));
        assert_eq!(
            ExecutionRef::parse(".")
                .unwrap()
                .resolve(Some(&c), Some(&p))
                .unwrap()
                .canonical(),
            c.canonical()
        );
        assert_eq!(
            ExecutionRef::parse("^")
                .unwrap()
                .resolve(Some(&c), Some(&p))
                .unwrap()
                .canonical(),
            p.canonical()
        );
        let child = ExecutionRef::parse("a:2/t:3/j:9")
            .unwrap()
            .resolve(Some(&c), None)
            .unwrap();
        assert_eq!(child.shortest_contextual_from(&c), "a:2/t:3/j:9");
    }

    #[test]
    fn rejects_aliases_backtracking_duplicates_and_noncanonical_nodes() {
        for invalid in [
            "a:2/p:x",
            "p:x/p:y",
            "t:1/a:2",
            "j:1/n:0",
            "j:1/n:01",
            "n:1",
            "t:1/n:2",
            "t:1/t:2",
            "j:1/t:2",
            "t:1/j:2/t:3",
            "..",
            "^/t:1",
        ] {
            assert!(ExecutionRef::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn refs_and_execution_depth_are_bounded() {
        let oversized = format!("a:{}", "a".repeat(MAX_REF_BYTES));
        assert_eq!(ExecutionRef::parse(&oversized), Err(RefError::RefTooLong));

        let deep = std::iter::once("t:1".to_owned())
            .chain((0..MAX_SUFFIX_SEGMENTS).map(|index| format!("j:{index}")))
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(ExecutionRef::parse(&deep), Err(RefError::TooManySegments));

        let suffix = std::iter::once(ExecutionSegment::Turn(h("1")))
            .chain(
                (0..MAX_SUFFIX_SEGMENTS)
                    .map(|index| ExecutionSegment::Job(h(&format!("j{index}")))),
            )
            .collect::<Vec<_>>();
        assert_eq!(
            ResolvedExecutionRef::new(current(), suffix),
            Err(RefError::TooManySegments)
        );
    }

    #[test]
    fn revision_is_nonzero_and_orthogonal_to_ref() {
        assert_eq!(Revision::new(0), Err(RefError::InvalidRevision));
        assert_eq!(Revision::new(9).unwrap().get(), 9);
    }
}
