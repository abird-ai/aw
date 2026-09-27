//! Narrow-only portable authority ceiling. Never an OS sandbox or a substitute
//! for host-authenticated provider, Session and filesystem policy.
use super::capability::{validate_actor_site, Capability, Effect, Grant, Source, SourceKind};
use alloc::{collections::BTreeSet, string::String, vec::Vec};

pub const MAX_GRANTS: usize = super::capability::MAX_CAPABILITIES;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Network {
    Deny,
    Allowlisted,
    AllowAll,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidAuthority,
    DuplicateGrant,
    TooManyGrants,
    WiderThanParent,
    InvalidBudget,
    NoPermission,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub max_model_rounds: u32,
    pub max_tool_calls: u32,
    pub max_children: u16,
    pub max_depth: u8,
    pub max_output_bytes: usize,
}
impl Budget {
    pub fn valid(self) -> bool {
        self.max_model_rounds > 0
            && self.max_model_rounds <= 32
            && self.max_tool_calls <= 128
            && self.max_children <= 64
            && self.max_depth <= 8
            && self.max_output_bytes > 0
            && self.max_output_bytes <= 1024 * 1024
    }
    pub fn fits_within(self, ceiling: Self) -> bool {
        self.max_model_rounds <= ceiling.max_model_rounds
            && self.max_tool_calls <= ceiling.max_tool_calls
            && self.max_children <= ceiling.max_children
            && self.max_depth <= ceiling.max_depth
            && self.max_output_bytes <= ceiling.max_output_bytes
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactGrant {
    pub name: String,
    pub kind: SourceKind,
    pub source: String,
    pub source_epoch: u64,
    pub effect: Effect,
}
impl ExactGrant {
    pub fn validate(&self, actor: &str, site: &str) -> Result<(), Error> {
        let source = Source::new(self.kind, &self.source, self.source_epoch)
            .map_err(|_| Error::InvalidAuthority)?;
        let cap = Capability::new(&self.name, source, self.effect)
            .map_err(|_| Error::InvalidAuthority)?;
        Grant::new(actor, site, cap).map_err(|_| Error::InvalidAuthority)?;
        Ok(())
    }
    pub fn permits(&self, actor: &str, site: &str, requested: &Self) -> bool {
        if self != requested {
            return false;
        }
        let source = Source::new(self.kind, &self.source, self.source_epoch);
        source
            .ok()
            .and_then(|s| Capability::new(&self.name, s, self.effect).ok())
            .and_then(|c| Grant::new(actor, site, c).ok())
            .is_some_and(|grant| {
                let src = Source::new(requested.kind, &requested.source, requested.source_epoch);
                src.ok()
                    .and_then(|s| Capability::new(&requested.name, s, requested.effect).ok())
                    .is_some_and(|req| grant.permits(actor, site, &req))
            })
    }
}
/// A requested root or child ceiling, not executable authority. Only Envelope
/// is validated; a raw spec must never be used for permission decisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeSpec {
    pub actor: String,
    pub site: String,
    pub revision: u64,
    pub access: Access,
    pub network: Network,
    pub process: bool,
    pub tty: bool,
    pub microphone: bool,
    pub speaker: bool,
    pub budget: Budget,
    pub grants: Vec<ExactGrant>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    inner: EnvelopeSpec,
}
impl Envelope {
    pub fn new(spec: EnvelopeSpec) -> Result<Self, Error> {
        if spec.revision == 0 || !spec.budget.valid() {
            return Err(Error::InvalidBudget);
        }
        if spec.grants.len() > MAX_GRANTS {
            return Err(Error::TooManyGrants);
        }
        // A zero-tool Envelope still needs a valid host principal and site.
        validate_actor_site(&spec.actor, &spec.site).map_err(|_| Error::InvalidAuthority)?;
        if spec.tty && !spec.process {
            return Err(Error::InvalidAuthority);
        }
        let mut seen = BTreeSet::new();
        for g in &spec.grants {
            g.validate(&spec.actor, &spec.site)?;
            if !seen.insert((
                g.name.as_str(),
                g.kind,
                g.source.as_str(),
                g.source_epoch,
                g.effect,
            )) {
                return Err(Error::DuplicateGrant);
            }
        }
        Ok(Self { inner: spec })
    }
    pub fn actor(&self) -> &str {
        &self.inner.actor
    }
    pub fn site(&self) -> &str {
        &self.inner.site
    }
    pub fn revision(&self) -> u64 {
        self.inner.revision
    }
    pub fn budget(&self) -> Budget {
        self.inner.budget
    }
    pub fn spec(&self) -> &EnvelopeSpec {
        &self.inner
    }
    pub fn allows(&self, grant: &ExactGrant) -> bool {
        self.inner
            .grants
            .iter()
            .any(|g| g.permits(self.actor(), self.site(), grant))
    }
    /// Reject even requested *potential* widening. The child is not an
    /// authorization source: the authenticated host must also verify lineage.
    pub fn narrow(&self, request: EnvelopeSpec) -> Result<Self, Error> {
        let p = &self.inner;
        if request.site != p.site
            || !request.budget.valid()
            || !request.budget.fits_within(p.budget)
            || (p.access == Access::ReadOnly && request.access == Access::ReadWrite)
            || request.network > p.network
            || (request.process && !p.process)
            || (request.tty && !p.tty)
            || (request.microphone && !p.microphone)
            || (request.speaker && !p.speaker)
            || request.grants.iter().any(|g| !self.allows(g))
        {
            return Err(Error::WiderThanParent);
        }
        Self::new(request)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    fn budget() -> Budget {
        Budget {
            max_model_rounds: 8,
            max_tool_calls: 24,
            max_children: 4,
            max_depth: 3,
            max_output_bytes: 2048,
        }
    }
    fn read() -> ExactGrant {
        ExactGrant {
            name: "read".into(),
            kind: SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: Effect::Read,
        }
    }
    fn spec(actor: &str) -> EnvelopeSpec {
        EnvelopeSpec {
            actor: actor.into(),
            site: "local".into(),
            revision: 1,
            access: Access::ReadWrite,
            network: Network::Deny,
            process: true,
            tty: true,
            microphone: false,
            speaker: false,
            budget: budget(),
            grants: vec![read()],
        }
    }
    #[test]
    fn narrow_does_not_union_authority() {
        let root = Envelope::new(spec("local")).unwrap();
        let mut child = spec("child");
        child.access = Access::ReadOnly;
        child.process = false;
        child.tty = false;
        child.revision = 2;
        let narrower = root.narrow(child.clone()).unwrap();
        assert!(narrower.allows(&read()));
        let mut wider = child.clone();
        wider.access = Access::ReadWrite;
        assert_eq!(narrower.narrow(wider), Err(Error::WiderThanParent));
        let mut bigger = child.clone();
        bigger.budget.max_tool_calls = 25;
        assert_eq!(root.narrow(bigger), Err(Error::WiderThanParent)); // parent call limit=24
        let mut write = read();
        write.effect = Effect::Write;
        child.grants = vec![write.clone()];
        assert_eq!(root.narrow(child), Err(Error::WiderThanParent));
        assert!(!root.allows(&write));
    }
    #[test]
    fn source_epoch_and_name_must_match() {
        let root = Envelope::new(spec("local")).unwrap();
        let mut other = read();
        other.source_epoch = 2;
        assert!(!root.allows(&other));
        other = read();
        other.name = "write".into();
        assert!(!root.allows(&other));
        let mut invalid = spec("");
        invalid.grants.clear();
        assert_eq!(Envelope::new(invalid), Err(Error::InvalidAuthority));
        let mut bad = spec("local");
        bad.grants.push(read());
        assert_eq!(Envelope::new(bad), Err(Error::DuplicateGrant));
        let mut location = spec("child");
        location.site = "remote".into();
        assert_eq!(root.narrow(location), Err(Error::WiderThanParent));
    }
}
