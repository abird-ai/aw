//! One portable authority predicate for trusted host, MCP and extension tools.
//! Registration/discovery is data; only an exact host-issued grant permits use.
pub const MAX_CAPABILITIES: usize = 64;
pub const MAX_NAME_BYTES: usize = 64;
pub const MAX_SOURCE_BYTES: usize = 128;
pub const MAX_ACTOR_BYTES: usize = 128;
pub const MAX_SITE_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKind {
    Host,
    Mcp,
    Extension,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    Read,
    Write,
    Process,
    /// Never assume untrusted read-only hints are authority.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidName,
    InvalidSource,
    InvalidActor,
    InvalidSite,
    InvalidEpoch,
    InvalidCatalogRevision,
    EmptyCatalog,
    TooManyCapabilities,
    DuplicateCapability,
}

fn valid_label(text: &str, max: usize, symbols: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || symbols.contains(&b))
}

/// The revision is minted by the trusted host, never by untrusted capability text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source<'a> {
    pub kind: SourceKind,
    pub id: &'a str,
    pub epoch: u64,
}
impl<'a> Source<'a> {
    pub fn new(kind: SourceKind, id: &'a str, epoch: u64) -> Result<Self, Error> {
        if !valid_label(id, MAX_SOURCE_BYTES, b"_.:-") {
            return Err(Error::InvalidSource);
        }
        if epoch == 0 {
            return Err(Error::InvalidEpoch);
        }
        Ok(Self { kind, id, epoch })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability<'a> {
    pub name: &'a str,
    pub source: Source<'a>,
    pub effect: Effect,
}
impl<'a> Capability<'a> {
    pub fn new(name: &'a str, source: Source<'a>, effect: Effect) -> Result<Self, Error> {
        if !valid_label(name, MAX_NAME_BYTES, b"_-") {
            return Err(Error::InvalidName);
        }
        let source = Source::new(source.kind, source.id, source.epoch)?;
        Ok(Self {
            name,
            source,
            effect,
        })
    }
}

/// Immutable borrowed view; no allocation, and no hidden authorization.
/// Small bounded tables use simple quadratic collision validation.
#[derive(Clone, Copy, Debug)]
pub struct Catalog<'a> {
    revision: u64,
    entries: &'a [Capability<'a>],
}
impl<'a> Catalog<'a> {
    pub fn new(revision: u64, entries: &'a [Capability<'a>]) -> Result<Self, Error> {
        if revision == 0 {
            return Err(Error::InvalidCatalogRevision);
        }
        if entries.is_empty() {
            return Err(Error::EmptyCatalog);
        }
        if entries.len() > MAX_CAPABILITIES {
            return Err(Error::TooManyCapabilities);
        }
        for (i, item) in entries.iter().enumerate() {
            Capability::new(item.name, item.source, item.effect)?;
            if entries[..i].iter().any(|prior| prior.name == item.name) {
                return Err(Error::DuplicateCapability);
            }
        }
        Ok(Self { revision, entries })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn find(&self, name: &str) -> Option<&Capability<'a>> {
        self.entries.iter().find(|item| item.name == name)
    }
    pub fn entries(&self) -> &[Capability<'a>] {
        self.entries
    }
}

/// Validate host principal and executor-site identifiers even for an actor
/// whose tool grant set is empty. Identity is never inferred from text.
pub fn validate_actor_site(actor: &str, site: &str) -> Result<(), Error> {
    if !valid_label(actor, MAX_ACTOR_BYTES, b":._-@") {
        return Err(Error::InvalidActor);
    }
    if site.is_empty() || site.len() > MAX_SITE_BYTES || site.chars().any(char::is_control) {
        return Err(Error::InvalidSite);
    }
    Ok(())
}

/// Explicit host-issued grant for exact actor, site, source epoch and effect.
/// Grant from a name alone must NEVER authorize an MCP/extension capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant<'a> {
    pub actor: &'a str,
    pub site: &'a str,
    pub capability: Capability<'a>,
}
impl<'a> Grant<'a> {
    pub fn new(actor: &'a str, site: &'a str, capability: Capability<'a>) -> Result<Self, Error> {
        validate_actor_site(actor, site)?;
        Capability::new(capability.name, capability.source, capability.effect)?;
        Ok(Self {
            actor,
            site,
            capability,
        })
    }
    pub fn permits(&self, actor: &str, site: &str, capability: &Capability<'_>) -> bool {
        self.actor == actor && self.site == site && self.capability == *capability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn desc<'a>(name: &'a str, source: &'a str, epoch: u64, effect: Effect) -> Capability<'a> {
        Capability::new(
            name,
            Source::new(SourceKind::Mcp, source, epoch).unwrap(),
            effect,
        )
        .unwrap()
    }
    #[test]
    fn exact_source_epoch_effect_actor_and_site_are_required() {
        let capability = desc("mcp__files__read", "files", 42, Effect::Read);
        let grant = Grant::new("local:user", "host:local", capability).unwrap();
        assert!(grant.permits("local:user", "host:local", &capability));
        assert!(!grant.permits("other", "host:local", &capability));
        assert!(!grant.permits("local:user", "host:remote", &capability));
        assert!(!grant.permits(
            "local:user",
            "host:local",
            &desc("mcp__files__read", "other", 42, Effect::Read)
        ));
        assert!(!grant.permits(
            "local:user",
            "host:local",
            &desc("mcp__files__read", "files", 43, Effect::Read)
        ));
        assert!(!grant.permits(
            "local:user",
            "host:local",
            &desc("mcp__files__read", "files", 42, Effect::Write)
        ));
        assert!(!grant.permits(
            "local:user",
            "host:local",
            &desc("mcp__files__read", "files", 42, Effect::Unknown)
        ));
        assert!(!grant.permits(
            "local:user",
            "host:local",
            &desc("mcp__files__write", "files", 42, Effect::Read)
        ));
    }
    #[test]
    fn bounded_catalog_rejects_duplicates_and_untrusted_labels() {
        let one = desc("read", "files", 1, Effect::Read);
        let two = desc("write", "files", 1, Effect::Write);
        let entries = [one, two];
        let cat = Catalog::new(8, &entries).unwrap();
        assert_eq!(cat.revision(), 8);
        assert_eq!(cat.find("write"), Some(&two));
        assert!(cat.find("unknown").is_none());
        assert!(matches!(
            Catalog::new(8, &[one, one]),
            Err(Error::DuplicateCapability)
        ));
        assert!(matches!(
            Catalog::new(0, &entries),
            Err(Error::InvalidCatalogRevision)
        ));
        assert!(matches!(Catalog::new(1, &[]), Err(Error::EmptyCatalog)));
        assert_eq!(
            Capability::new("bad/name", one.source, Effect::Read),
            Err(Error::InvalidName)
        );
        assert_eq!(
            Source::new(SourceKind::Extension, "../escape", 1),
            Err(Error::InvalidSource)
        );
        assert_eq!(
            Source::new(SourceKind::Extension, "plugin", 0),
            Err(Error::InvalidEpoch)
        );
        assert_eq!(Grant::new("", "local", one), Err(Error::InvalidActor));
        assert_eq!(Grant::new("user", "", one), Err(Error::InvalidSite));
    }
    #[test]
    fn bounded_catalog_requires_no_heap() {
        let item = desc("read", "files", 1, Effect::Read);
        let entries = [item; MAX_CAPABILITIES + 1];
        assert!(matches!(
            Catalog::new(1, &entries),
            Err(Error::TooManyCapabilities)
        ));
        assert!(matches!(
            Catalog::new(1, &entries[..2]),
            Err(Error::DuplicateCapability)
        ));
    }
}
