//! Canonical DL-200 Event subject identity.
//!
//! Concrete subjects are a reversible dot-delimited projection of the existing
//! hierarchical resource refs. This module owns syntax only; matching and
//! authorization remain separate Event-layer concerns.

use crate::execution_ref::{Handle, NodeCoordinate, MAX_REF_BYTES};

pub const MAX_EVENT_SUBJECT_BYTES: usize = 2048;
pub const MAX_EVENT_NAME_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventSubjectError {
    TooLong,
    InvalidSyntax,
    InvalidHierarchy,
    InvalidHandle,
    InvalidNodeCoordinate,
    InvalidEventName,
    NonCanonicalEncoding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Domain,
    Identity,
    Project,
    Sandbox,
    Agent,
    Turn,
    Job,
    Node,
}

impl Kind {
    fn parse(value: &str) -> Result<Self, EventSubjectError> {
        match value {
            "d" => Ok(Self::Domain),
            "i" => Ok(Self::Identity),
            "p" => Ok(Self::Project),
            "s" => Ok(Self::Sandbox),
            "a" => Ok(Self::Agent),
            "t" => Ok(Self::Turn),
            "j" => Ok(Self::Job),
            "n" => Ok(Self::Node),
            _ => Err(EventSubjectError::InvalidSyntax),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventSubject {
    raw: String,
    source_ref: String,
    event_name: String,
}

impl EventSubject {
    pub fn parse(value: &str) -> Result<Self, EventSubjectError> {
        if value.is_empty() || value.len() > MAX_EVENT_SUBJECT_BYTES || !value.is_ascii() {
            return Err(if value.len() > MAX_EVENT_SUBJECT_BYTES {
                EventSubjectError::TooLong
            } else {
                EventSubjectError::InvalidSyntax
            });
        }
        let (source, event_name) = value
            .rsplit_once(".e:")
            .ok_or(EventSubjectError::InvalidHierarchy)?;
        validate_event_name(event_name)?;
        if source.is_empty() {
            return Err(EventSubjectError::InvalidHierarchy);
        }

        let mut kinds = [Kind::Domain; 8];
        let mut kind_count = 0usize;
        let mut source_ref = String::with_capacity(value.len().min(MAX_REF_BYTES));
        for token in source.split('.') {
            let (kind, encoded) = token
                .split_once(':')
                .ok_or(EventSubjectError::InvalidSyntax)?;
            if kind.len() != 1 || encoded.is_empty() || encoded.contains(':') {
                return Err(EventSubjectError::InvalidSyntax);
            }
            let kind = Kind::parse(kind)?;
            let slot = kinds
                .get_mut(kind_count)
                .ok_or(EventSubjectError::InvalidHierarchy)?;
            *slot = kind;
            kind_count += 1;
            source_ref.push('/');
            source_ref.push(match kind {
                Kind::Domain => 'd',
                Kind::Identity => 'i',
                Kind::Project => 'p',
                Kind::Sandbox => 's',
                Kind::Agent => 'a',
                Kind::Turn => 't',
                Kind::Job => 'j',
                Kind::Node => 'n',
            });
            source_ref.push(':');
            if kind == Kind::Node {
                let coordinate = parse_node(encoded)?;
                source_ref.push_str(&coordinate.to_string());
            } else {
                source_ref.push_str(&decode_handle(encoded)?);
            }
        }
        validate_hierarchy(&kinds[..kind_count])?;
        if source_ref.len() > MAX_REF_BYTES {
            return Err(EventSubjectError::TooLong);
        }

        Ok(Self {
            raw: value.to_owned(),
            source_ref,
            event_name: event_name.to_owned(),
        })
    }

    pub fn from_resource_ref(
        resource_ref: &str,
        event_name: &str,
    ) -> Result<Self, EventSubjectError> {
        validate_event_name(event_name)?;
        if resource_ref.is_empty()
            || resource_ref.len() > MAX_REF_BYTES
            || !resource_ref.starts_with('/')
            || resource_ref.ends_with('/')
            || resource_ref.contains("//")
            || !resource_ref.is_ascii()
        {
            return Err(EventSubjectError::InvalidSyntax);
        }

        let mut kinds = [Kind::Domain; 8];
        let mut kind_count = 0usize;
        let mut subject = String::with_capacity(resource_ref.len() + event_name.len() + 4);
        for segment in resource_ref[1..].split('/') {
            let (kind, value) = segment
                .split_once(':')
                .ok_or(EventSubjectError::InvalidSyntax)?;
            if kind.len() != 1 || value.is_empty() || value.contains(':') {
                return Err(EventSubjectError::InvalidSyntax);
            }
            let kind = Kind::parse(kind)?;
            let slot = kinds
                .get_mut(kind_count)
                .ok_or(EventSubjectError::InvalidHierarchy)?;
            *slot = kind;
            kind_count += 1;
            if !subject.is_empty() {
                subject.push('.');
            }
            subject.push(match kind {
                Kind::Domain => 'd',
                Kind::Identity => 'i',
                Kind::Project => 'p',
                Kind::Sandbox => 's',
                Kind::Agent => 'a',
                Kind::Turn => 't',
                Kind::Job => 'j',
                Kind::Node => 'n',
            });
            subject.push(':');
            if kind == Kind::Node {
                let coordinate = parse_node(value)?;
                subject.push_str(&coordinate.to_string());
            } else {
                let handle =
                    Handle::new(value.to_owned()).map_err(|_| EventSubjectError::InvalidHandle)?;
                encode_handle(handle.as_str(), &mut subject);
            }
        }
        validate_hierarchy(&kinds[..kind_count])?;
        subject.push_str(".e:");
        subject.push_str(event_name);
        if subject.len() > MAX_EVENT_SUBJECT_BYTES {
            return Err(EventSubjectError::TooLong);
        }
        Self::parse(&subject)
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn source_ref(&self) -> &str {
        &self.source_ref
    }

    pub fn event_name(&self) -> &str {
        &self.event_name
    }

    pub fn into_string(self) -> String {
        self.raw
    }
}

/// One already-contextualized absolute Event selector.
///
/// Contextual expansion belongs to the host edge; the shared matcher sees only
/// the canonical typed namespace. Matching is allocation-free after parse.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventPattern {
    raw: String,
}

impl EventPattern {
    pub fn parse_absolute(value: &str) -> Result<Self, EventSubjectError> {
        if value.is_empty() || value.len() > MAX_EVENT_SUBJECT_BYTES || !value.is_ascii() {
            return Err(if value.len() > MAX_EVENT_SUBJECT_BYTES {
                EventSubjectError::TooLong
            } else {
                EventSubjectError::InvalidSyntax
            });
        }

        let mut kinds = [Kind::Domain; 8];
        let mut kind_count = 0usize;
        let mut tokens = value.split('.').peekable();
        while let Some(token) = tokens.next() {
            if token == ">" {
                if tokens.next().is_some() || kind_count == 0 {
                    return Err(EventSubjectError::InvalidHierarchy);
                }
                validate_hierarchy(&kinds[..kind_count])?;
                return Ok(Self {
                    raw: value.to_owned(),
                });
            }

            let (prefix, encoded) = token
                .split_once(':')
                .ok_or(EventSubjectError::InvalidSyntax)?;
            if prefix == "e" {
                if tokens.next().is_some() || kind_count == 0 {
                    return Err(EventSubjectError::InvalidHierarchy);
                }
                validate_hierarchy(&kinds[..kind_count])?;
                if encoded != "*" {
                    validate_event_name(encoded)?;
                }
                return Ok(Self {
                    raw: value.to_owned(),
                });
            }

            if prefix.len() != 1 || encoded.is_empty() || encoded.contains(':') {
                return Err(EventSubjectError::InvalidSyntax);
            }
            let kind = Kind::parse(prefix)?;
            let slot = kinds
                .get_mut(kind_count)
                .ok_or(EventSubjectError::InvalidHierarchy)?;
            *slot = kind;
            kind_count += 1;
            if encoded != "*" {
                if kind == Kind::Node {
                    parse_node(encoded)?;
                } else {
                    decode_handle(encoded)?;
                }
            }
        }

        Err(EventSubjectError::InvalidHierarchy)
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Exact Agent root that wholly contains this selector, when one exists.
    ///
    /// This is syntax only: placement and current authority remain host/store
    /// concerns. Broad selectors return None rather than claiming completeness.
    pub fn exact_agent_ref(&self) -> Option<String> {
        let mut tokens = self.raw.split('.');
        let mut out = String::with_capacity(self.raw.len().min(MAX_REF_BYTES));
        for expected in ["d", "i", "p", "a"] {
            let token = tokens.next()?;
            let (kind, encoded) = token.split_once(':')?;
            if kind != expected || encoded == "*" {
                return None;
            }
            let decoded = decode_handle(encoded).ok()?;
            out.push('/');
            out.push_str(expected);
            out.push(':');
            out.push_str(&decoded);
        }
        Some(out)
    }

    /// Exact Project root for selectors whose Agent component is one-token wildcard.
    ///
    /// This admits broad observation only across Agent descendants of one concrete
    /// Project. Project-level Events and broader Domain/Principal/Project wildcards
    /// deliberately remain outside this placement shape.
    pub fn wildcard_agent_project_ref(&self) -> Option<String> {
        let mut tokens = self.raw.split('.');
        let mut out = String::with_capacity(self.raw.len().min(MAX_REF_BYTES));
        for expected in ["d", "i", "p"] {
            let token = tokens.next()?;
            let (kind, encoded) = token.split_once(':')?;
            if kind != expected || encoded == "*" {
                return None;
            }
            let decoded = decode_handle(encoded).ok()?;
            out.push('/');
            out.push_str(expected);
            out.push(':');
            out.push_str(&decoded);
        }
        let (kind, encoded) = tokens.next()?.split_once(':')?;
        (kind == "a" && encoded == "*").then_some(out)
    }

    pub fn matches_canonical(&self, subject: &str) -> bool {
        let pattern = self.raw.split('.');
        let mut subject = subject.split('.');
        for pattern_token in pattern {
            if pattern_token == ">" {
                return subject.next().is_some();
            }
            let Some(subject_token) = subject.next() else {
                return false;
            };
            let Some((pattern_kind, pattern_value)) = pattern_token.split_once(':') else {
                return false;
            };
            if pattern_value == "*" {
                let Some((subject_kind, subject_value)) = subject_token.split_once(':') else {
                    return false;
                };
                if pattern_kind != subject_kind || subject_value.is_empty() {
                    return false;
                }
            } else if pattern_token != subject_token {
                return false;
            }
        }
        subject.next().is_none()
    }
}

fn validate_hierarchy(kinds: &[Kind]) -> Result<(), EventSubjectError> {
    use Kind::{Agent, Domain, Identity, Job, Node, Project, Sandbox, Turn};
    match kinds {
        [Domain]
        | [Domain, Identity]
        | [Domain, Identity, Project]
        | [Domain, Identity, Project, Sandbox]
        | [Domain, Identity, Project, Agent]
        | [Domain, Identity, Project, Agent, Turn]
        | [Domain, Identity, Project, Agent, Turn, Job]
        | [Domain, Identity, Project, Agent, Turn, Job, Node] => Ok(()),
        _ => Err(EventSubjectError::InvalidHierarchy),
    }
}

fn parse_node(value: &str) -> Result<NodeCoordinate, EventSubjectError> {
    if value.len() > 1 && value.starts_with('0')
        || value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(EventSubjectError::InvalidNodeCoordinate);
    }
    let value = value
        .parse::<u32>()
        .map_err(|_| EventSubjectError::InvalidNodeCoordinate)?;
    NodeCoordinate::new(value).map_err(|_| EventSubjectError::InvalidNodeCoordinate)
}

fn encode_handle(value: &str, out: &mut String) {
    for byte in value.bytes() {
        if byte == b'.' {
            out.push_str("%2E");
        } else {
            out.push(byte as char);
        }
    }
}

fn decode_handle(value: &str) -> Result<String, EventSubjectError> {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if bytes.get(index + 1..index + 3) != Some(b"2E") {
                    return Err(EventSubjectError::NonCanonicalEncoding);
                }
                decoded.push('.');
                index += 3;
            }
            byte if byte.is_ascii() => {
                decoded.push(byte as char);
                index += 1;
            }
            _ => return Err(EventSubjectError::InvalidHandle),
        }
    }
    let handle = Handle::new(decoded.clone()).map_err(|_| EventSubjectError::InvalidHandle)?;
    let mut canonical = String::with_capacity(value.len());
    encode_handle(handle.as_str(), &mut canonical);
    if canonical != value {
        return Err(EventSubjectError::NonCanonicalEncoding);
    }
    Ok(decoded)
}

fn validate_event_name(value: &str) -> Result<(), EventSubjectError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_EVENT_NAME_BYTES {
        return Err(EventSubjectError::InvalidEventName);
    }
    if let Some(custom) = value.strip_prefix('#') {
        let bytes = custom.as_bytes();
        if bytes.is_empty()
            || !bytes[0].is_ascii_lowercase()
            || !bytes[1..].iter().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'_' | b'-')
            })
        {
            return Err(EventSubjectError::InvalidEventName);
        }
        return Ok(());
    }
    if !bytes[0].is_ascii_alphanumeric()
        || !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'~'))
    {
        return Err(EventSubjectError::InvalidEventName);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_patterns_keep_typed_nats_cardinality_without_partial_globs() {
        let subtree = EventPattern::parse_absolute("d:acme.i:alice.p:core.a:*.>").unwrap();
        assert_eq!(subtree.exact_agent_ref(), None);
        assert_eq!(
            subtree.wildcard_agent_project_ref().as_deref(),
            Some("/d:acme/i:alice/p:core")
        );
        let exact_agent = EventPattern::parse_absolute("d:acme.i:alice.p:foo%2Ebar.a:2.>").unwrap();
        assert_eq!(
            exact_agent.exact_agent_ref().as_deref(),
            Some("/d:acme/i:alice/p:foo.bar/a:2")
        );
        assert_eq!(exact_agent.wildcard_agent_project_ref(), None);
        assert_eq!(
            EventPattern::parse_absolute("d:acme.i:alice.p:core.e:*")
                .unwrap()
                .wildcard_agent_project_ref(),
            None
        );
        assert!(subtree.matches_canonical("d:acme.i:alice.p:core.a:2.e:changed"));
        assert!(subtree.matches_canonical("d:acme.i:alice.p:core.a:2.t:7.j:test.e:result"));
        assert!(!subtree.matches_canonical("d:acme.i:alice.p:other.a:2.e:changed"));

        let direct = EventPattern::parse_absolute("d:acme.i:alice.p:core.a:*.e:*").unwrap();
        assert!(direct.matches_canonical("d:acme.i:alice.p:core.a:2.e:#ready"));
        assert!(!direct.matches_canonical("d:acme.i:alice.p:core.a:2.t:7.e:settled"));

        let dotted =
            EventPattern::parse_absolute("d:acme.i:alice.p:foo%2Ebar.a:*.e:changed").unwrap();
        assert!(dotted.matches_canonical("d:acme.i:alice.p:foo%2Ebar.a:2.e:changed"));

        for invalid in [
            "d:acme.p:*.e:*",
            "d:acme.i:alice.p:core.a:*.e:#*",
            "d:acme.i:alice.p:foo%2ebar.a:*.e:*",
            "d:acme.i:alice.p:core.a:*.>.e:changed",
            "d:acme.i:alice.p:core.a:*",
        ] {
            assert!(EventPattern::parse_absolute(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn concrete_subjects_round_trip_typed_resource_refs_and_encode_literal_dots() {
        let cases = [
            ("/d:acme", "changed", "d:acme.e:changed"),
            (
                "/d:acme/i:compiler-team",
                "changed",
                "d:acme.i:compiler-team.e:changed",
            ),
            (
                "/d:acme/i:alice/p:core/s:dev",
                "changed",
                "d:acme.i:alice.p:core.s:dev.e:changed",
            ),
            (
                "/d:acme/i:alice/p:foo.bar/a:2/t:7/j:test/n:4",
                "#ready",
                "d:acme.i:alice.p:foo%2Ebar.a:2.t:7.j:test.n:4.e:#ready",
            ),
        ];
        for (resource, event, subject) in cases {
            let projected = EventSubject::from_resource_ref(resource, event).unwrap();
            assert_eq!(projected.as_str(), subject);
            assert_eq!(projected.source_ref(), resource);
            assert_eq!(projected.event_name(), event);
            assert_eq!(EventSubject::parse(subject).unwrap(), projected);
        }

        for invalid in [
            "d:acme.p:core.e:changed",
            "d:acme.i:alice.p:core.t:7.e:settled",
            "d:acme.i:alice.p:foo%2ebar.a:2.e:changed",
            "d:acme.i:alice.p:foo%252Ebar.a:2.e:changed",
            "d:acme.i:alice.p:core.a:2.e:#Ready",
            "d:acme.i:alice.p:core.a:2.n:1.e:changed",
        ] {
            assert!(EventSubject::parse(invalid).is_err(), "{invalid}");
        }
    }
}
