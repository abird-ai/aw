//! Built-in Git SourceDriver. Blocking, trusted-host operations; never a tool
//! grant. The caller owns durable Prepared custody and the source mutation
//! barrier through publication. This slice captures committed clean sources;
//! dirty trees and submodules fail closed until their exact semantics land.

use crate::{valid_object_id, Error as GitError, Repository};
use aw_agent_core::{
    CheckpointPolicy, DriverArtifact, DriverKey, DriverNamespace, DriverSchemaPin, Error,
    ExternalEvidence, PreparedSourceOperation, RecoveryDescriptor, RecoveryField,
    RecoveryFieldKind, RecoveryInspection, Result, SchemaSupport, SourceAccess, SourceBinding,
    SourceDriverHandle, SourceDriverRegistry, SourceDriverSchemas, SourceName, SourceOperationKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const BINDING_SCHEMA: u32 = 1;
const ANCHOR_SCHEMA: u32 = 1;
const RECOVERY_SCHEMA: u32 = 1;

fn key() -> DriverKey {
    DriverKey {
        namespace: DriverNamespace::Builtin,
        local_id: "git".into(),
    }
}

fn schema(version: u32) -> SchemaSupport {
    SchemaSupport {
        compatible_from: version,
        current: version,
    }
}

fn git_error(error: GitError) -> Error {
    match error {
        GitError::Limit(reason) => Error::Limit(reason),
        GitError::Unsupported(reason) => Error::Denied(reason),
        GitError::Stale => Error::Conflict("Git source changed"),
        GitError::Timeout | GitError::MutationUnknown => {
            Error::Conflict("Git source effect uncertain; inspect the Prepared operation")
        }
        _ => Error::Storage("Git source unavailable or invalid"),
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_oid(value: &str) -> bool {
    valid_object_id(value) && !value.bytes().any(|b| b.is_ascii_uppercase())
}

fn validate_source_ref(value: &str) -> Result<()> {
    if !value.strip_prefix("git:").is_some_and(is_digest) {
        return Err(Error::Denied("invalid Git source identity"));
    }
    Ok(())
}

fn validate_checkpoint_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
    {
        return Err(Error::Config("invalid Git checkpoint identity".into()));
    }
    Ok(())
}

fn retention_ref(checkpoint_id: &str, name: &SourceName) -> String {
    format!("refs/aw/checkpoints/{checkpoint_id}/{}", name.as_str())
}

fn materialization_parts(binding: &SourceBinding) -> Result<(&str, &Path)> {
    binding.validate()?;
    if binding.driver != key() || binding.binding_schema != BINDING_SCHEMA {
        return Err(Error::Denied("incompatible Git SourceBinding"));
    }
    validate_source_ref(&binding.source_ref)?;
    let (stamp, path) = binding
        .materialization
        .split_once(':')
        .ok_or(Error::Denied("invalid Git materialization identity"))?;
    if !is_digest(stamp) || !Path::new(path).is_absolute() {
        return Err(Error::Denied("invalid Git materialization identity"));
    }
    Ok((stamp, Path::new(path)))
}

fn hash_part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

// Local directory identities are observations, not authority or live leases.
// A replaced repository/checkout cannot be adopted merely by reusing its path.
fn repository_stamps(repo: &Repository) -> Result<(String, String)> {
    repo.validate_binding().map_err(git_error)?;
    let identity = repo
        .identity
        .as_ref()
        .ok_or(Error::Storage("Git directory identity unavailable"))?;
    // A ref in a borrowing repository does not protect objects from GC in
    // its alternate object database. Require locally owned object storage.
    let objects = repo.common.join("objects");
    let objects_id = aw_platform::DirectoryIdentity::open(&objects)?;
    for path in ["info/alternates", "info/http-alternates"] {
        match std::fs::symlink_metadata(objects.join(path)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {
                return Err(Error::Denied(
                    "Git source refuses alternate object databases",
                ))
            }
        }
    }
    let mut source = Sha256::new();
    source.update(b"aw-git-source-v1\0");
    hash_directory(&mut source, &repo.common, &identity.common)?;
    hash_directory(&mut source, &objects, &objects_id)?;
    let source = format!("git:{:x}", source.finalize());

    let mut checkout = Sha256::new();
    checkout.update(b"aw-git-checkout-v1\0");
    hash_part(&mut checkout, source.as_bytes());
    for (path, dir) in [
        (&repo.root, &identity.root),
        (&repo.git_dir, &identity.git_dir),
    ] {
        hash_directory(&mut checkout, path, dir)?;
    }
    Ok((source, format!("{:x}", checkout.finalize())))
}

fn hash_directory(
    hash: &mut Sha256,
    path: &Path,
    identity: &aw_platform::DirectoryIdentity,
) -> Result<()> {
    let (volume, object) = identity.identity()?;
    hash.update(volume.to_be_bytes());
    hash.update(object.to_be_bytes());
    hash_part(
        hash,
        path.to_str()
            .ok_or(Error::Denied("Git source requires UTF-8 paths"))?
            .as_bytes(),
    );
    Ok(())
}

/// Immutable Git content identity plus the AW-owned retention reference.
/// The reference is object custody, not a source/disclosure grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSourceAnchor {
    pub driver: DriverKey,
    pub anchor_schema: u32,
    pub source_ref: String,
    pub name: SourceName,
    pub access: SourceAccess,
    pub checkpoint_policy: CheckpointPolicy,
    pub checkpoint_id: String,
    pub base_commit: String,
    pub captured_commit: String,
}

impl GitSourceAnchor {
    pub fn pin(&self) -> Result<DriverSchemaPin> {
        DriverSchemaPin::new(
            self.driver.clone(),
            DriverArtifact::Anchor,
            self.anchor_schema,
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.name.validate()?;
        validate_source_ref(&self.source_ref)?;
        validate_checkpoint_id(&self.checkpoint_id)?;
        if self.driver != key() || self.anchor_schema != ANCHOR_SCHEMA {
            return Err(Error::Denied("incompatible Git SourceAnchor"));
        }
        if !is_oid(&self.base_commit) || self.captured_commit != self.base_commit {
            return Err(Error::Denied("unsupported Git anchor identity"));
        }
        Ok(())
    }

    pub fn retention_ref(&self) -> String {
        retention_ref(&self.checkpoint_id, &self.name)
    }
}

struct CapturePlan {
    binding: SourceBinding,
    checkpoint_id: String,
    base: String,
}

impl CapturePlan {
    fn fields(&self) -> Result<Vec<RecoveryField>> {
        use RecoveryFieldKind::{Id, Reference, Version};
        let binding = &self.binding;
        let access = match binding.access {
            SourceAccess::ReadOnly => "ro",
            SourceAccess::ReadWrite => "rw",
        };
        let policy = match binding.checkpoint_policy {
            CheckpointPolicy::Required => "required",
            CheckpointPolicy::Ephemeral => "ephemeral",
        };
        [
            ("checkpoint", Id, self.checkpoint_id.as_str()),
            ("name", Id, binding.name.as_str()),
            ("source", Reference, binding.source_ref.as_str()),
            (
                "materialization",
                Reference,
                binding.materialization.as_str(),
            ),
            ("base", Version, self.base.as_str()),
            ("access", Id, access),
            ("checkpoint_policy", Id, policy),
        ]
        .into_iter()
        .map(|(name, kind, value)| RecoveryField::new(name, kind, value))
        .collect()
    }

    fn prepare(&self, prepare_id: &str) -> Result<PreparedSourceOperation> {
        let fields = self.fields()?;
        let mut hash = Sha256::new();
        hash.update(b"aw-git-capture-v1\0");
        hash_part(&mut hash, prepare_id.as_bytes());
        for field in &fields {
            hash_part(&mut hash, field.name.as_bytes());
            hash_part(&mut hash, field.value.as_bytes());
        }
        PreparedSourceOperation::new(
            SourceOperationKind::Capture,
            RecoveryDescriptor::new(
                key(),
                RECOVERY_SCHEMA,
                prepare_id,
                format!("{:x}", hash.finalize()),
                fields,
            )?,
        )
    }

    fn decode(prepared: &PreparedSourceOperation) -> Result<Self> {
        prepared.validate()?;
        let descriptor = &prepared.descriptor;
        if prepared.operation != SourceOperationKind::Capture
            || descriptor.driver != key()
            || descriptor.recovery_schema != RECOVERY_SCHEMA
        {
            return Err(Error::Denied("incompatible Git capture descriptor"));
        }
        let field = |name| {
            descriptor
                .fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| field.value.as_str())
                .ok_or(Error::Storage("missing Git capture descriptor field"))
        };
        let binding = SourceBinding {
            name: SourceName::new(field("name")?)?,
            driver: key(),
            binding_schema: BINDING_SCHEMA,
            source_ref: field("source")?.into(),
            materialization: field("materialization")?.into(),
            access: match field("access")? {
                "ro" => SourceAccess::ReadOnly,
                "rw" => SourceAccess::ReadWrite,
                _ => return Err(Error::Storage("invalid Git capture access")),
            },
            checkpoint_policy: match field("checkpoint_policy")? {
                "required" => CheckpointPolicy::Required,
                "ephemeral" => CheckpointPolicy::Ephemeral,
                _ => return Err(Error::Storage("invalid Git capture policy")),
            },
        };
        materialization_parts(&binding)?;
        let checkpoint_id = field("checkpoint")?.to_owned();
        validate_checkpoint_id(&checkpoint_id)?;
        let base = field("base")?.to_owned();
        if !is_oid(&base) {
            return Err(Error::Storage("invalid Git capture commit"));
        }
        let plan = Self {
            binding,
            checkpoint_id,
            base,
        };
        // One canonical descriptor encoding; reject extra/changed fields,
        // kinds and prepared identity instead of interpreting another schema.
        if plan.prepare(&descriptor.prepare_id)? != *prepared {
            return Err(Error::Storage(
                "Git capture descriptor does not match its identity",
            ));
        }
        Ok(plan)
    }

    fn anchor(&self) -> GitSourceAnchor {
        GitSourceAnchor {
            driver: key(),
            anchor_schema: ANCHOR_SCHEMA,
            source_ref: self.binding.source_ref.clone(),
            name: self.binding.name.clone(),
            access: self.binding.access,
            checkpoint_policy: self.binding.checkpoint_policy,
            checkpoint_id: self.checkpoint_id.clone(),
            base_commit: self.base.clone(),
            captured_commit: self.base.clone(),
        }
    }
}

// Disable replacement objects for exact content identity. Existing Repository
// runner owns all other subprocess containment, bounded IO and config policy.
fn run(repo: &Repository, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = Vec::with_capacity(args.len() + 1);
    command.push("--no-replace-objects");
    command.extend_from_slice(args);
    repo.run(&command, None, false).map_err(git_error)
}

fn head(repo: &Repository) -> Result<String> {
    let output = run(repo, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let value = std::str::from_utf8(&output)
        .ok()
        .and_then(|s| s.strip_suffix('\n'))
        .filter(|s| is_oid(s))
        .ok_or(Error::Storage("invalid Git HEAD"))?;
    Ok(value.to_owned())
}

fn clean_head(repo: &Repository) -> Result<String> {
    repo.guard_managed_checkout().map_err(git_error)?;
    let base = head(repo)?;
    // Do not run nested Git status/filters until gitlink semantics land.
    // Check both committed and indexed trees so ignore-submodules cannot
    // conceal a staged gitlink add/delete.
    let tree = run(repo, &["ls-tree", "-r", "-z", &base])?;
    if tree
        .split(|b| *b == 0)
        .any(|entry| entry.starts_with(b"160000 "))
    {
        return Err(Error::Denied("Git submodule capture is not yet supported"));
    }
    let index = run(repo, &["ls-files", "-v", "--stage", "-z"])?;
    for entry in index.split(|b| *b == 0).filter(|entry| !entry.is_empty()) {
        let metadata = entry.split(|b| *b == b'\t').next().unwrap_or_default();
        if !metadata.starts_with(b"H ")
            || metadata.starts_with(b"H 160000 ")
            || !metadata.ends_with(b" 0")
        {
            return Err(Error::Denied(
                "Git capture refuses gitlinks, conflicts or hidden index entries",
            ));
        }
    }
    if !run(
        repo,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    )?
    .is_empty()
    {
        return Err(Error::Conflict("dirty Git capture is not yet supported"));
    }
    if head(repo)? != base {
        return Err(Error::Conflict(
            "Git HEAD changed during capture preparation",
        ));
    }
    repo.validate_binding().map_err(git_error)?;
    Ok(base)
}

// Exact private reference lookup; refuse symbolic refs and ambiguous descendants.
fn retained_commit(repo: &Repository, reference: &str) -> Result<Option<String>> {
    let output = run(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname) %(symref)",
            reference,
        ],
    )?;
    if output.is_empty() {
        return Ok(None);
    }
    let line = std::str::from_utf8(&output)
        .ok()
        .and_then(|s| s.strip_suffix('\n'))
        .ok_or(Error::Storage("invalid Git retention reference"))?;
    let mut parts = line.split(' ');
    let actual = parts.next();
    let object = parts.next().filter(|s| is_oid(s));
    if actual != Some(reference) || parts.next() != Some("") || parts.next().is_some() {
        return Err(Error::Conflict(
            "Git retention reference changed or is ambiguous",
        ));
    }
    Ok(Some(
        object
            .ok_or(Error::Storage("invalid Git retained object"))?
            .to_owned(),
    ))
}

fn verify_retained(repo: &Repository, anchor: &GitSourceAnchor) -> Result<()> {
    anchor.validate()?;
    repo.guard_local_content().map_err(git_error)?;
    if repository_stamps(repo)?.0 != anchor.source_ref
        || retained_commit(repo, &anchor.retention_ref())?.as_deref()
            != Some(&anchor.captured_commit)
    {
        return Err(Error::Conflict(
            "Git anchor retention is unavailable or changed",
        ));
    }
    verify_commit_tree(repo, &anchor.captured_commit)
}

fn verify_commit_tree(repo: &Repository, commit: &str) -> Result<()> {
    if run(repo, &["cat-file", "-t", commit])? != b"commit\n" {
        return Err(Error::Storage("Git retained object is not a commit"));
    }
    // Check this exact snapshot's reachable tree/blob objects, not all history.
    // Quiet traversal avoids transferring the object list into our wire buffer.
    run(
        repo,
        &[
            "rev-list",
            "--no-walk",
            "--objects",
            "--quiet",
            "--missing=error",
            commit,
        ],
    )?;
    Ok(())
}

/// Concrete built-in registration. The host chooses an absolute trusted Git
/// executable and authorized repositories; registry publication grants nothing.
pub struct GitSourceDriver {
    git: PathBuf,
}

impl GitSourceDriver {
    pub fn new(git: impl AsRef<Path>) -> Result<Self> {
        let git = git.as_ref();
        if !git.is_absolute() {
            return Err(Error::Config(
                "absolute trusted Git executable required".into(),
            ));
        }
        let git = git.canonicalize()?;
        if !git.is_file() {
            return Err(Error::Config("trusted Git executable unavailable".into()));
        }
        Ok(Self { git })
    }

    pub fn register(self, registry: &SourceDriverRegistry) -> Result<SourceDriverHandle<Self>> {
        registry.register(
            key(),
            SourceDriverSchemas {
                binding: schema(BINDING_SCHEMA),
                anchor: schema(ANCHOR_SCHEMA),
                recovery: schema(RECOVERY_SCHEMA),
            },
            self,
        )
    }

    fn repository(&self, path: &Path) -> Result<Repository> {
        Repository::discover(&self.git, path).map_err(git_error)
    }

    pub fn binding(
        &self,
        path: &Path,
        name: SourceName,
        access: SourceAccess,
        checkpoint_policy: CheckpointPolicy,
    ) -> Result<SourceBinding> {
        let repo = self.repository(path)?;
        repo.guard_managed_checkout().map_err(git_error)?;
        let (source_ref, stamp) = repository_stamps(&repo)?;
        let path = repo
            .root()
            .to_str()
            .ok_or(Error::Denied("Git binding requires a UTF-8 path"))?;
        let binding = SourceBinding {
            name,
            driver: key(),
            binding_schema: BINDING_SCHEMA,
            source_ref,
            materialization: format!("{stamp}:{path}"),
            access,
            checkpoint_policy,
        };
        binding.validate()?;
        Ok(binding)
    }

    fn binding_repository(&self, binding: &SourceBinding) -> Result<Repository> {
        let (stamp, path) = materialization_parts(binding)?;
        let repo = self.repository(path)?;
        let (source, current_stamp) = repository_stamps(&repo)?;
        if repo.root() != path || source != binding.source_ref || current_stamp != stamp {
            return Err(Error::Conflict(
                "Git source or materialization identity changed",
            ));
        }
        Ok(repo)
    }

    /// Read-only planning. Persist this exact descriptor in the existing
    /// operation receipt before executing. The checkpoint ID is owner-minted.
    pub fn plan_capture(
        &self,
        binding: &SourceBinding,
        checkpoint_id: &str,
        prepare_id: &str,
    ) -> Result<PreparedSourceOperation> {
        validate_checkpoint_id(checkpoint_id)?;
        let repo = self.binding_repository(binding)?;
        let reference = retention_ref(checkpoint_id, &binding.name);
        if retained_commit(&repo, &reference)?.is_some() {
            return Err(Error::Conflict(
                "Git retention identity already exists; inspect its operation",
            ));
        }
        let base = clean_head(&repo)?;
        verify_commit_tree(&repo, &base)?;
        CapturePlan {
            binding: binding.clone(),
            checkpoint_id: checkpoint_id.into(),
            base,
        }
        .prepare(prepare_id)
    }

    /// Execute only a durably Prepared, first-attempt operation under the
    /// caller's source barrier. A lost response must go to inspect, not here.
    pub fn execute_capture(&self, prepared: &PreparedSourceOperation) -> Result<GitSourceAnchor> {
        let plan = CapturePlan::decode(prepared)?;
        let repo = self.binding_repository(&plan.binding)?;
        let anchor = plan.anchor();
        let reference = anchor.retention_ref();
        if retained_commit(&repo, &reference)?.is_some() {
            return Err(Error::Conflict(
                "Git capture already has retention; inspect the Prepared operation",
            ));
        }
        if clean_head(&repo)? != plan.base {
            return Err(Error::Conflict(
                "Git source changed after capture preparation",
            ));
        }
        verify_commit_tree(&repo, &plan.base)?;
        // The only external effect in the clean path: create, never overwrite.
        let absent = "0".repeat(plan.base.len());
        run(
            &repo,
            &[
                "-c",
                "core.logAllRefUpdates=false",
                "update-ref",
                "--no-deref",
                &reference,
                &plan.base,
                &absent,
            ],
        )
        .map_err(|_| {
            Error::Conflict("Git retention outcome uncertain; inspect the Prepared operation")
        })?;
        verify_retained(&repo, &anchor)?;
        Ok(anchor)
    }

    /// Inspect exactly the prepared retention identity; no creation, retry,
    /// deletion or reinterpretation. An absent ref alone does not prove that a
    /// prior effect never happened (e.g. it could have been removed).
    pub fn inspect_capture(
        &self,
        prepared: &PreparedSourceOperation,
    ) -> Result<RecoveryInspection<GitSourceAnchor>> {
        let plan = CapturePlan::decode(prepared)?;
        let anchor = plan.anchor();
        if self
            .binding_repository(&plan.binding)
            .and_then(|repo| verify_retained(&repo, &anchor))
            .is_err()
        {
            return Ok(RecoveryInspection::Unknown);
        }
        // The commit OID is immutable content evidence, NOT an immutable
        // lease on the ref. The AW owner must pin retention and hold its
        // source mutation/cleanup barrier through the publication transaction.
        let evidence = ExternalEvidence::immutable(
            "git_commit",
            format!("{}:{}", anchor.source_ref, anchor.captured_commit),
        )?;
        RecoveryInspection::completed(anchor, evidence)
    }

    /// Recheck the exact capture boundary and retained object before the
    /// caller's final commit; the caller must still hold its live AW barrier.
    pub fn revalidate_capture(
        &self,
        prepared: &PreparedSourceOperation,
        anchor: &GitSourceAnchor,
    ) -> Result<()> {
        let plan = CapturePlan::decode(prepared)?;
        if plan.anchor() != *anchor {
            return Err(Error::Conflict("Git anchor differs from prepared capture"));
        }
        let repo = self.binding_repository(&plan.binding)?;
        if clean_head(&repo)? != plan.base {
            return Err(Error::Conflict("Git source changed after capture"));
        }
        verify_retained(&repo, anchor)
    }

    /// Read-only retention validation independent of the current HEAD. A
    /// different authorized checkout may access the same object database.
    pub fn verify_anchor(&self, repository: &Path, anchor: &GitSourceAnchor) -> Result<()> {
        verify_retained(&self.repository(repository)?, anchor)
    }
}
