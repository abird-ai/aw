//! Typed SourceDriver definition registry and durable recovery vocabulary.
//!
//! This leaf owns implementation identity/compatibility only. Source authority,
//! bindings, checkpoints, Prepared operation custody and publication transactions
//! remain with their durable Store/host owners.

use crate::{
    publication,
    registry::{self, StagedRegistry},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

pub const DEFAULT_MAX_SOURCES_PER_SANDBOX: usize = 128;
pub const HARD_MAX_SOURCES_PER_SANDBOX: usize = 1024;
const MAX_SOURCE_DRIVERS: usize = 1024;
const MAX_SOURCE_NAME_BYTES: usize = 64;
const MAX_SOURCE_BINDING_IDENTITY_BYTES: usize = 2048;
const MAX_PREPARE_ID_BYTES: usize = 128;
const MAX_SOURCE_FIELDS: usize = 32;
const MAX_SOURCE_FIELD_NAME_BYTES: usize = 48;
const MAX_SOURCE_FIELD_VALUE_BYTES: usize = 2048;
/// Bounded encoded driver-specific anchor fields (including JSON escaping).
pub const MAX_SOURCE_ANCHOR_FIELDS_BYTES: usize = 256 * 1024;
const MAX_EVIDENCE_KIND_BYTES: usize = 48;
const MAX_EVIDENCE_VALUE_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum DriverNamespace {
    Builtin,
    Extension(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverKey {
    pub namespace: DriverNamespace,
    pub local_id: String,
}

impl DriverKey {
    pub fn builtin(local_id: impl Into<String>) -> Result<Self> {
        let key = Self {
            namespace: DriverNamespace::Builtin,
            local_id: local_id.into(),
        };
        key.validate()?;
        Ok(key)
    }

    pub fn extension(extension: impl Into<String>, local_id: impl Into<String>) -> Result<Self> {
        let key = Self {
            namespace: DriverNamespace::Extension(extension.into()),
            local_id: local_id.into(),
        };
        key.validate()?;
        Ok(key)
    }

    pub fn validate(&self) -> Result<()> {
        registry::validate_local_id(&self.local_id)?;
        if let DriverNamespace::Extension(extension) = &self.namespace {
            registry::validate_extension_id(extension)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverArtifact {
    Binding,
    Anchor,
    Recovery,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverSchemaPin {
    pub driver: DriverKey,
    pub artifact: DriverArtifact,
    pub schema_version: u32,
}

impl DriverSchemaPin {
    pub fn new(driver: DriverKey, artifact: DriverArtifact, schema_version: u32) -> Result<Self> {
        let pin = Self {
            driver,
            artifact,
            schema_version,
        };
        pin.validate()?;
        Ok(pin)
    }

    pub fn validate(&self) -> Result<()> {
        self.driver.validate()?;
        if self.schema_version == 0 {
            return Err(Error::Config(
                "driver artifact schema version must be positive".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaSupport {
    pub compatible_from: u32,
    pub current: u32,
}

impl SchemaSupport {
    pub fn new(compatible_from: u32, current: u32) -> Result<Self> {
        let support = Self {
            compatible_from,
            current,
        };
        support.validate()?;
        Ok(support)
    }

    pub fn validate(self) -> Result<()> {
        if self.compatible_from == 0 || self.current == 0 || self.compatible_from > self.current {
            return Err(Error::Config(
                "driver schema support requires 1 <= compatible_from <= current".into(),
            ));
        }
        Ok(())
    }

    pub const fn supports(self, schema_version: u32) -> bool {
        schema_version >= self.compatible_from && schema_version <= self.current
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDriverSchemas {
    pub binding: SchemaSupport,
    pub anchor: SchemaSupport,
    pub recovery: SchemaSupport,
}

impl SourceDriverSchemas {
    pub fn validate(self) -> Result<()> {
        self.binding.validate()?;
        self.anchor.validate()?;
        self.recovery.validate()
    }

    pub fn support(self, artifact: DriverArtifact) -> SchemaSupport {
        match artifact {
            DriverArtifact::Binding => self.binding,
            DriverArtifact::Anchor => self.anchor,
            DriverArtifact::Recovery => self.recovery,
        }
    }

    pub fn supports(self, pin: &DriverSchemaPin) -> bool {
        self.support(pin.artifact).supports(pin.schema_version)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceName(String);

impl SourceName {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_source_name(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn validate(&self) -> Result<()> {
        validate_source_name(&self.0)
    }
}

fn validate_source_name(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_SOURCE_NAME_BYTES
        || !bytes[0].is_ascii_lowercase()
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'_' | b'-')
        })
    {
        return Err(Error::Config(
            "source name requires [a-z][a-z0-9_-]{0,63}".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAccess {
    ReadOnly,
    ReadWrite,
}

impl SourceAccess {
    /// Intersection of independently granted maxima. Absence of a grant
    /// is represented by None, never by ReadOnly.
    #[inline]
    pub const fn intersect(self, other: Self) -> Self {
        match (self, other) {
            (Self::ReadWrite, Self::ReadWrite) => Self::ReadWrite,
            _ => Self::ReadOnly,
        }
    }

    #[inline]
    pub const fn permits(self, requested: Self) -> bool {
        matches!(self, Self::ReadWrite) || matches!(requested, Self::ReadOnly)
    }
}

/// Explicit named ceiling, independent of the materialized binding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedSourceAccess {
    pub name: SourceName,
    pub access: SourceAccess,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointPolicy {
    #[default]
    Required,
    Ephemeral,
}

/// A named, already-materialized source. References are non-secret driver-issued
/// identities/locators, never credential URLs or authority to use the source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub name: SourceName,
    pub driver: DriverKey,
    pub binding_schema: u32,
    pub source_ref: String,
    pub materialization: String,
    pub access: SourceAccess,
    #[serde(default)]
    pub checkpoint_policy: CheckpointPolicy,
}

impl SourceBinding {
    pub fn pin(&self) -> Result<DriverSchemaPin> {
        DriverSchemaPin::new(
            self.driver.clone(),
            DriverArtifact::Binding,
            self.binding_schema,
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.name.validate()?;
        self.pin()?;
        for value in [&self.source_ref, &self.materialization] {
            validate_identity(
                value,
                MAX_SOURCE_BINDING_IDENTITY_BYTES,
                "source binding identity",
            )?;
            if value.contains("://") {
                return Err(Error::Config(
                    "source binding requires an opaque reference, not a URL".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSetLimit {
    max_sources: usize,
}

impl Default for SourceSetLimit {
    fn default() -> Self {
        Self {
            max_sources: DEFAULT_MAX_SOURCES_PER_SANDBOX,
        }
    }
}

impl SourceSetLimit {
    pub fn new(max_sources: usize) -> Result<Self> {
        if max_sources == 0 || max_sources > DEFAULT_MAX_SOURCES_PER_SANDBOX {
            return Err(Error::Limit(
                "host source limit must be 1..=128 and may only lower the default",
            ));
        }
        Ok(Self { max_sources })
    }

    pub const fn max_sources(self) -> usize {
        self.max_sources
    }

    pub fn admit(self, count: usize) -> Result<()> {
        validate_source_set_hard_limit(count)?;
        if count > self.max_sources {
            return Err(Error::Limit("source set exceeds the host source bound"));
        }
        Ok(())
    }
}

pub fn validate_source_set_hard_limit(count: usize) -> Result<()> {
    if count > HARD_MAX_SOURCES_PER_SANDBOX {
        return Err(Error::Limit(
            "source set exceeds the absolute 1024-source safety ceiling",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOperationKind {
    Capture,
    Restore,
    Derive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceIdentityFieldKind {
    Id,
    Digest,
    Version,
    Reference,
    OpaqueHandle,
}

pub use SourceIdentityFieldKind as RecoveryFieldKind;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentityField {
    pub name: String,
    pub kind: SourceIdentityFieldKind,
    pub value: String,
}

pub use SourceIdentityField as RecoveryField;

impl SourceIdentityField {
    pub fn new(
        name: impl Into<String>,
        kind: SourceIdentityFieldKind,
        value: impl Into<String>,
    ) -> Result<Self> {
        let field = Self {
            name: name.into(),
            kind,
            value: value.into(),
        };
        field.validate()?;
        Ok(field)
    }

    pub fn validate(&self) -> Result<()> {
        let name = self.name.as_bytes();
        if name.is_empty()
            || name.len() > MAX_SOURCE_FIELD_NAME_BYTES
            || !name[0].is_ascii_lowercase()
            || !name.iter().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'_' | b'-')
            })
        {
            return Err(Error::Config("invalid source identity field name".into()));
        }
        if self.value.is_empty()
            || self.value.len() > MAX_SOURCE_FIELD_VALUE_BYTES
            || self.value.chars().any(char::is_control)
        {
            return Err(Error::Config(
                "source field must be bounded non-control identity evidence".into(),
            ));
        }
        Ok(())
    }
}

fn validate_identity_fields(fields: &[SourceIdentityField]) -> Result<()> {
    if fields.len() > MAX_SOURCE_FIELDS {
        return Err(Error::Limit("too many source identity fields"));
    }
    let mut names = std::collections::BTreeSet::new();
    for field in fields {
        field.validate()?;
        if !names.insert(field.name.as_str()) {
            return Err(Error::Config(
                "source identity field names must be unique".into(),
            ));
        }
    }
    Ok(())
}

/// Immutable, driver-issued source version and retention identity. Layout
/// metadata records captured maxima, never authority. Fields are non-secret
/// version/provenance IDs, not credentials or a mutable materialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceAnchor {
    pub name: SourceName,
    pub driver: DriverKey,
    pub anchor_schema: u32,
    pub source_ref: String,
    pub access: SourceAccess,
    pub checkpoint_policy: CheckpointPolicy,
    pub identity: String,
    pub retention: String,
    pub fields: Vec<SourceIdentityField>,
}

impl SourceAnchor {
    pub fn pin(&self) -> Result<DriverSchemaPin> {
        DriverSchemaPin::new(
            self.driver.clone(),
            DriverArtifact::Anchor,
            self.anchor_schema,
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.name.validate()?;
        self.pin()?;
        for value in [&self.source_ref, &self.identity, &self.retention] {
            validate_identity(
                value,
                MAX_SOURCE_FIELD_VALUE_BYTES,
                "source anchor identity",
            )?;
            if value.contains("://") {
                return Err(Error::Config(
                    "source anchor requires opaque identities, not URLs".into(),
                ));
            }
        }
        validate_identity_fields(&self.fields)?;
        if self.fields.iter().any(|field| field.value.contains("://")) {
            return Err(Error::Config(
                "source anchor requires opaque fields, not URLs".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCoverage {
    AutoAllRequired,
    Selected,
    None,
}

/// One checkpoint's immutable named SourceSet. A lowered host limit affects
/// new admission, not decoding a previously retained set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSet {
    pub coverage: SourceCoverage,
    pub anchors: Vec<SourceAnchor>,
}

impl SourceSet {
    pub fn validate(&self) -> Result<()> {
        validate_source_set_hard_limit(self.anchors.len())?;
        match self.coverage {
            SourceCoverage::None if !self.anchors.is_empty() => {
                return Err(Error::Config("history-only SourceSet must be empty".into()));
            }
            SourceCoverage::Selected if self.anchors.is_empty() => {
                return Err(Error::Config(
                    "selected SourceSet requires named sources".into(),
                ));
            }
            _ => {}
        }
        let mut names = std::collections::BTreeSet::new();
        for anchor in &self.anchors {
            anchor.validate()?;
            if !names.insert(anchor.name.as_str()) {
                return Err(Error::Conflict("duplicate SourceAnchor name"));
            }
            if self.coverage == SourceCoverage::AutoAllRequired
                && anchor.checkpoint_policy != CheckpointPolicy::Required
            {
                return Err(Error::Config(
                    "automatic SourceSet may contain only required sources".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn admit(&self, limit: SourceSetLimit) -> Result<()> {
        limit.admit(self.anchors.len())?;
        self.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ExternalEvidence {
    Immutable {
        evidence_kind: String,
        identity: String,
    },
    HeldFence {
        provider: String,
        handle: String,
    },
}

impl ExternalEvidence {
    pub fn immutable(
        evidence_kind: impl Into<String>,
        identity: impl Into<String>,
    ) -> Result<Self> {
        let evidence = Self::Immutable {
            evidence_kind: evidence_kind.into(),
            identity: identity.into(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    /// Durable identity for a provider-held fence only. The handle is not a
    /// capability and does not prove publication safety by itself; the live
    /// provider must still hold the corresponding fence through AW commit.
    pub fn held_fence(provider: impl Into<String>, handle: impl Into<String>) -> Result<Self> {
        let evidence = Self::HeldFence {
            provider: provider.into(),
            handle: handle.into(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<()> {
        let (kind, value) = match self {
            Self::Immutable {
                evidence_kind,
                identity,
            } => (evidence_kind.as_str(), identity.as_str()),
            Self::HeldFence { provider, handle } => (provider.as_str(), handle.as_str()),
        };
        validate_evidence_component(kind, MAX_EVIDENCE_KIND_BYTES)?;
        validate_evidence_component(value, MAX_EVIDENCE_VALUE_BYTES)
    }
}

/// A provider/driver-owned live guard. Its owner must guarantee the exact
/// represented evidence and external authority through the entire publication
/// transaction, until this guard is dropped. A validity check, opaque handle,
/// TTL or deserialized receipt is not an implementation of that guarantee.
/// Implement only at the trusted provider/driver boundary.
pub trait SourcePublicationFence {
    fn evidence(&self) -> &ExternalEvidence;
}

// Immutable-only implementations need no fictitious fence.
impl SourcePublicationFence for std::convert::Infallible {
    fn evidence(&self) -> &ExternalEvidence {
        match *self {}
    }
}

/// Revalidated evidence from the trusted driver. Immutable is appropriate
/// only when both the fact and the necessary external authority cannot be
/// invalidated during commit. Held owns a real provider guard (including
/// when immutable content has revocable authority). Never serialized.
pub enum SourcePublicationProof<F> {
    Immutable(ExternalEvidence),
    Held(F),
}

impl<F: SourcePublicationFence> SourcePublicationProof<F> {
    /// Bind live proof to the exact inspected identity. The driver must also
    /// have checked the original descriptor/result and capture fingerprint.
    pub fn require_evidence(&self, expected: &ExternalEvidence) -> Result<()> {
        let actual = match self {
            Self::Immutable(evidence) => {
                if !matches!(evidence, ExternalEvidence::Immutable { .. }) {
                    return Err(Error::Denied(
                        "revocable source evidence requires a live fence",
                    ));
                }
                evidence
            }
            Self::Held(fence) => fence.evidence(),
        };
        actual.validate()?;
        if actual != expected {
            return Err(Error::Conflict(
                "source publication evidence identity changed",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryDescriptor {
    pub driver: DriverKey,
    pub recovery_schema: u32,
    pub prepare_id: String,
    pub prepared_identity: String,
    pub fields: Vec<RecoveryField>,
}

impl RecoveryDescriptor {
    pub fn new(
        driver: DriverKey,
        recovery_schema: u32,
        prepare_id: impl Into<String>,
        prepared_identity: impl Into<String>,
        fields: Vec<RecoveryField>,
    ) -> Result<Self> {
        let descriptor = Self {
            driver,
            recovery_schema,
            prepare_id: prepare_id.into(),
            prepared_identity: prepared_identity.into(),
            fields,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn pin(&self) -> Result<DriverSchemaPin> {
        DriverSchemaPin::new(
            self.driver.clone(),
            DriverArtifact::Recovery,
            self.recovery_schema,
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.driver.validate()?;
        if self.recovery_schema == 0 {
            return Err(Error::Config(
                "recovery descriptor schema must be positive".into(),
            ));
        }
        validate_identity(&self.prepare_id, MAX_PREPARE_ID_BYTES, "prepare identity")?;
        validate_identity(
            &self.prepared_identity,
            MAX_SOURCE_FIELD_VALUE_BYTES,
            "prepared source identity",
        )?;
        validate_identity_fields(&self.fields)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedSourceOperation {
    pub operation: SourceOperationKind,
    pub descriptor: RecoveryDescriptor,
}

impl PreparedSourceOperation {
    pub fn new(operation: SourceOperationKind, descriptor: RecoveryDescriptor) -> Result<Self> {
        let prepared = Self {
            operation,
            descriptor,
        };
        prepared.validate()?;
        Ok(prepared)
    }

    pub fn validate(&self) -> Result<()> {
        self.descriptor.validate()
    }
}

/// One named driver's exact prepared operation, owned by the enclosing
/// construction receipt, not a separately replayable resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedPreparedSource {
    pub name: SourceName,
    pub prepared: PreparedSourceOperation,
}

/// One operation-owned identity and bounded set of per-source descriptors.
/// All entries describe the same capture or restore. No saved evidence is a
/// live fence, and none of these fields is credential custody.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedSourceSet {
    pub prepare_id: String,
    pub operation: SourceOperationKind,
    pub sources: Vec<NamedPreparedSource>,
}

impl PreparedSourceSet {
    pub fn validate(&self) -> Result<()> {
        validate_identity(&self.prepare_id, MAX_PREPARE_ID_BYTES, "prepare identity")?;
        validate_source_set_hard_limit(self.sources.len())?;
        if self.sources.is_empty() || self.prepare_id.contains("://") {
            return Err(Error::Config(
                "invalid prepared source set identity or size".into(),
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        let mut effects = std::collections::BTreeSet::new();
        for source in &self.sources {
            source.name.validate()?;
            source.prepared.validate()?;
            let descriptor = &source.prepared.descriptor;
            if source.prepared.operation != self.operation
                || descriptor.prepare_id != self.prepare_id
            {
                return Err(Error::Conflict(
                    "source preparation belongs to another operation",
                ));
            }
            if !names.insert(source.name.as_str())
                || !effects.insert((&descriptor.driver, descriptor.prepared_identity.as_str()))
            {
                return Err(Error::Conflict(
                    "duplicate prepared source name or effect identity",
                ));
            }
            if descriptor.prepared_identity.contains("://")
                || descriptor
                    .fields
                    .iter()
                    .any(|field| field.value.contains("://"))
            {
                return Err(Error::Config(
                    "prepared sources require non-secret opaque identities, not URLs".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn admit(&self, limit: SourceSetLimit) -> Result<()> {
        limit.admit(self.sources.len())?;
        self.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryInspection<T> {
    NotStarted,
    Completed {
        result: T,
        evidence: ExternalEvidence,
    },
    Unknown,
}

impl<T> RecoveryInspection<T> {
    pub fn completed(result: T, evidence: ExternalEvidence) -> Result<Self> {
        evidence.validate()?;
        Ok(Self::Completed { result, evidence })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDriverDescriptor {
    pub driver: DriverKey,
    pub schemas: SourceDriverSchemas,
    pub registration_revision: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SourceDriverHandle<T> {
    driver: DriverKey,
    schemas: SourceDriverSchemas,
    registration_revision: u64,
    implementation: std::marker::PhantomData<fn() -> T>,
}

impl<T> Clone for SourceDriverHandle<T> {
    fn clone(&self) -> Self {
        Self {
            driver: self.driver.clone(),
            schemas: self.schemas,
            registration_revision: self.registration_revision,
            implementation: std::marker::PhantomData,
        }
    }
}

impl<T> SourceDriverHandle<T> {
    pub fn driver(&self) -> &DriverKey {
        &self.driver
    }

    pub const fn schemas(&self) -> SourceDriverSchemas {
        self.schemas
    }

    pub const fn registration_revision(&self) -> u64 {
        self.registration_revision
    }

    pub fn descriptor(&self) -> SourceDriverDescriptor {
        SourceDriverDescriptor {
            driver: self.driver.clone(),
            schemas: self.schemas,
            registration_revision: self.registration_revision,
        }
    }
}

#[derive(Clone)]
struct SourceDriverEntry {
    descriptor: SourceDriverDescriptor,
    implementation: registry::NativeDefinition,
}

#[derive(Clone, Default)]
struct SourceDriverSnapshot {
    entries: BTreeMap<DriverKey, SourceDriverEntry>,
}

fn validate_snapshot(snapshot: &SourceDriverSnapshot) -> Result<()> {
    if snapshot.entries.len() > MAX_SOURCE_DRIVERS {
        return Err(Error::Limit("too many registered SourceDrivers"));
    }
    for (key, entry) in &snapshot.entries {
        key.validate()?;
        if entry.descriptor.driver != *key || entry.descriptor.registration_revision == 0 {
            return Err(Error::Config(
                "invalid SourceDriver registry snapshot".into(),
            ));
        }
        validate_schemas(entry.descriptor.schemas)?;
    }
    Ok(())
}

fn validate_schemas(schemas: SourceDriverSchemas) -> Result<()> {
    schemas.validate()
}

#[derive(Clone)]
pub struct SourceDriverRegistry {
    owner: Arc<StagedRegistry<SourceDriverSnapshot>>,
}

pub struct SourceDriverRegistryReader {
    reader: publication::Reader<SourceDriverSnapshot>,
}

impl Default for SourceDriverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceDriverRegistry {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(StagedRegistry::new(SourceDriverSnapshot::default())),
        }
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.owner.current()?.generation)
    }

    pub fn reader(&self) -> Result<SourceDriverRegistryReader> {
        Ok(SourceDriverRegistryReader {
            reader: self.owner.reader_factory().reader()?,
        })
    }

    pub fn register<T: Send + Sync + 'static>(
        &self,
        driver: DriverKey,
        schemas: SourceDriverSchemas,
        implementation: T,
    ) -> Result<SourceDriverHandle<T>> {
        self.register_if(driver, schemas, implementation, None)
    }

    pub fn register_if<T: Send + Sync + 'static>(
        &self,
        driver: DriverKey,
        schemas: SourceDriverSchemas,
        implementation: T,
        expected_revision: Option<u64>,
    ) -> Result<SourceDriverHandle<T>> {
        driver.validate()?;
        validate_schemas(schemas)?;
        let implementation = registry::NativeDefinition::new(implementation);
        let stage = self.owner.stage(
            expected_revision,
            |current, next_revision| {
                if current.entries.contains_key(&driver) {
                    return Err(Error::Conflict("SourceDriver is already registered"));
                }
                let descriptor = SourceDriverDescriptor {
                    driver: driver.clone(),
                    schemas,
                    registration_revision: next_revision,
                };
                let mut next = current.clone();
                next.entries.insert(
                    driver.clone(),
                    SourceDriverEntry {
                        descriptor,
                        implementation,
                    },
                );
                Ok((
                    next,
                    SourceDriverHandle {
                        driver,
                        schemas,
                        registration_revision: next_revision,
                        implementation: std::marker::PhantomData,
                    },
                ))
            },
            validate_snapshot,
        )?;
        self.owner.publish(stage).map(|(_, handle)| handle)
    }

    pub fn list(&self) -> Result<Vec<SourceDriverDescriptor>> {
        let current = self.owner.current()?;
        Ok(current
            .value
            .entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}

impl SourceDriverRegistryReader {
    pub fn revision(&self) -> Result<u64> {
        Ok(self.reader.snapshot()?.generation)
    }

    /// Resolve a currently installed native definition by its stable key and
    /// concrete type. The returned handle pins this registration, not authority.
    pub fn handle<T: Send + Sync + 'static>(
        &self,
        driver: &DriverKey,
    ) -> Result<SourceDriverHandle<T>> {
        driver.validate()?;
        let snapshot = self.reader.snapshot()?;
        let entry = snapshot
            .value
            .entries
            .get(driver)
            .ok_or(Error::Denied("required SourceDriver is unavailable"))?;
        entry.implementation.get::<T>()?;
        Ok(SourceDriverHandle {
            driver: driver.clone(),
            schemas: entry.descriptor.schemas,
            registration_revision: entry.descriptor.registration_revision,
            implementation: std::marker::PhantomData,
        })
    }

    pub fn get<T: Send + Sync + 'static>(&self, handle: &SourceDriverHandle<T>) -> Result<Arc<T>> {
        let snapshot = self.reader.snapshot()?;
        let entry = snapshot
            .value
            .entries
            .get(&handle.driver)
            .ok_or(Error::Denied("SourceDriver handle is stale"))?;
        if entry.descriptor.registration_revision != handle.registration_revision
            || entry.descriptor.schemas != handle.schemas
        {
            return Err(Error::Denied("SourceDriver handle is stale"));
        }
        entry.implementation.get::<T>()
    }

    pub fn resolve(&self, pin: &DriverSchemaPin) -> Result<SourceDriverDescriptor> {
        pin.validate()?;
        let snapshot = self.reader.snapshot()?;
        let entry = snapshot
            .value
            .entries
            .get(&pin.driver)
            .ok_or(Error::Denied("required SourceDriver is unavailable"))?;
        if !entry.descriptor.schemas.supports(pin) {
            return Err(Error::Denied(
                "SourceDriver does not support the durable artifact schema",
            ));
        }
        Ok(entry.descriptor.clone())
    }

    pub fn list(&self) -> Result<Vec<SourceDriverDescriptor>> {
        let snapshot = self.reader.snapshot()?;
        Ok(snapshot
            .value
            .entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
}

fn validate_identity(value: &str, max: usize, label: &'static str) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::Config(format!("{label} is invalid")));
    }
    Ok(())
}

fn validate_evidence_component(value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::Config(
            "external evidence identity must be bounded and non-control".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct GitV1(&'static str);

    #[derive(Debug, PartialEq, Eq)]
    struct GitV2(&'static str);

    fn v1_schemas() -> SourceDriverSchemas {
        SourceDriverSchemas {
            binding: SchemaSupport::new(1, 1).unwrap(),
            anchor: SchemaSupport::new(1, 1).unwrap(),
            recovery: SchemaSupport::new(1, 1).unwrap(),
        }
    }

    #[test]
    fn source_driver_leaf_separates_durable_schema_identity_from_live_registration() {
        let driver = DriverKey::builtin("git").unwrap();
        let registry = SourceDriverRegistry::new();
        let first = registry
            .register(driver.clone(), v1_schemas(), GitV1("first"))
            .unwrap();
        let reader = registry.reader().unwrap();
        let installed = reader.handle::<GitV1>(&driver).unwrap();
        assert_eq!(installed.descriptor(), first.descriptor());
        assert!(Arc::ptr_eq(
            &reader.get(&installed).unwrap(),
            &reader.get(&first).unwrap(),
        ));
        assert!(reader.handle::<GitV2>(&driver).is_err());
        assert_eq!(reader.get(&first).unwrap().as_ref(), &GitV1("first"));

        let recovery = RecoveryDescriptor::new(
            driver.clone(),
            1,
            "prep_01",
            "repo:abc123",
            vec![
                RecoveryField::new("oid", RecoveryFieldKind::Digest, "abc123").unwrap(),
                RecoveryField::new(
                    "retention_ref",
                    RecoveryFieldKind::Reference,
                    "refs/aw/checkpoints/cp_1/code",
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let pin = recovery.pin().unwrap();
        assert_eq!(reader.resolve(&pin).unwrap().registration_revision, 1);

        let other = registry
            .register(
                DriverKey::extension("fixture", "object").unwrap(),
                SourceDriverSchemas {
                    binding: SchemaSupport::new(1, 2).unwrap(),
                    anchor: SchemaSupport::new(1, 2).unwrap(),
                    recovery: SchemaSupport::new(1, 2).unwrap(),
                },
                GitV2("other"),
            )
            .unwrap();
        assert_eq!(reader.get(&other).unwrap().as_ref(), &GitV2("other"));
        assert_eq!(
            reader.resolve(&pin).unwrap().registration_revision,
            first.registration_revision(),
            "unrelated leaf publication cannot stale durable DriverKey/schema resolution"
        );
        assert_eq!(reader.get(&first).unwrap().as_ref(), &GitV1("first"));

        let unsupported = DriverSchemaPin::new(driver, DriverArtifact::Recovery, 2).unwrap();
        assert!(reader.resolve(&unsupported).is_err());

        assert!(SourceSetLimit::default()
            .admit(DEFAULT_MAX_SOURCES_PER_SANDBOX)
            .is_ok());
        assert!(SourceSetLimit::default()
            .admit(DEFAULT_MAX_SOURCES_PER_SANDBOX + 1)
            .is_err());
        assert!(SourceSetLimit::new(DEFAULT_MAX_SOURCES_PER_SANDBOX + 1).is_err());
        assert!(validate_source_set_hard_limit(HARD_MAX_SOURCES_PER_SANDBOX).is_ok());
        assert!(validate_source_set_hard_limit(HARD_MAX_SOURCES_PER_SANDBOX + 1).is_err());

        let evidence = ExternalEvidence::held_fence("git", "fence_opaque_1").unwrap();
        assert!(matches!(evidence, ExternalEvidence::HeldFence { .. }));
        assert!(matches!(
            RecoveryInspection::completed("anchor", evidence).unwrap(),
            RecoveryInspection::Completed { .. }
        ));
    }
}
