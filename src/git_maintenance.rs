//! Shared local-host observations and approval lease for offline Git
//! maintenance. Pins and advisory locks are not physical containment.
pub(crate) use crate::private_approval::{
    canonical_private_directory, lock_private_approval, open_private_approval, overlaps,
};
use aw::{Error, Result};
use aw_agent_core::validate_source_set_hard_limit;
use aw_git::source_driver::{GitRestoreLocation, GitSourceLocation};
use aw_host::source_checkpoint::GitRestoreFence;
use aw_platform::DirectoryIdentity;
use aw_store::SessionStore;
use std::{fs::File, path::Path};

/// Canonical complete-set inputs, shared by the offline source owners.
pub(crate) fn validate_named_locations(sources: &mut [GitSourceLocation]) -> Result<()> {
    validate_source_set_hard_limit(sources.len())?;
    for source in sources.iter() {
        source.name.validate()?;
        if !source.repository.is_absolute() || source.repository.parent().is_none() {
            return Err(Error::Denied(
                "Git maintenance requires exact checkout roots",
            ));
        }
    }
    sources.sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
    if sources.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(Error::Denied("duplicate Git source approval"));
    }
    Ok(())
}

pub(crate) fn require_store_scope(
    store: &SessionStore,
    directory: &Path,
    id: &str,
    generation: u64,
) -> Result<()> {
    let process = store.local_process_info()?;
    if store.directory() != Some(directory)
        || store.store_id()? != id
        || process.store_generation != generation
        || process.process_kind != "owner_lock"
        || process.state != "active"
    {
        return Err(Error::Denied(
            "Git maintenance approval differs from current Store",
        ));
    }
    Ok(())
}

/// Do not initialize a missing Store through a maintenance command.
pub(crate) fn require_existing_store(path: &Path) -> Result<()> {
    canonical_private_directory(path)?;
    aw_platform::open_private_existing(&path.join("sessions.sqlite3"), false)?;
    Ok(())
}

/// Restrict this offline operator path to private ordinary checkouts with
/// contained Git administration. The driver still verifies exact backing
/// identity and the live operator must exclude aliases/external writers.
pub(crate) fn pin_private_checkout(
    location: &GitSourceLocation,
    protected: &[&Path],
) -> Result<[DirectoryIdentity; 3]> {
    let root = &location.repository;
    if protected.iter().any(|path| overlaps(root, path)) {
        return Err(Error::Denied(
            "approved checkout overlaps private host state",
        ));
    }
    canonical_private_directory(root)?;
    let pin = DirectoryIdentity::open(root)?;
    if pin.identity()? != location.repository_identity {
        return Err(Error::Conflict("approved Git checkout changed"));
    }
    let admin = root.join(".git");
    let objects = admin.join("objects");
    for path in [&admin, &objects] {
        if path.canonicalize()? != *path {
            return Err(Error::Denied("Git administration must be contained"));
        }
    }
    let admin_pin = DirectoryIdentity::open(&admin)?;
    let objects_pin = DirectoryIdentity::open(&objects)?;
    match std::fs::symlink_metadata(admin.join("commondir")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
        Ok(_) => return Err(Error::Denied("separate Git common directory not approved")),
    }
    Ok([pin, admin_pin, objects_pin])
}

/// Common lexical preflight for the approved private destination.
/// Physical identities are checked by the actual pinned layout below.
pub(crate) fn require_named_destination(
    root: &Path,
    cwd: &Path,
    store: &Path,
    approval_directory: &Path,
) -> Result<()> {
    if cwd != root
        || !root.is_absolute()
        || root.parent().is_none()
        || overlaps(approval_directory, store)
        || overlaps(root, approval_directory)
        || overlaps(root, store)
    {
        return Err(Error::Denied(
            "Git maintenance requires a disjoint private named layout",
        ));
    }
    Ok(())
}

/// Retain the original approval lease and exact directory observations as
/// the common physical fence for the existing Git maintenance consumers.
pub(crate) struct PinnedGitLayout<'a> {
    _lease: &'a File,
    locations: Vec<GitRestoreLocation>,
    pins: Vec<DirectoryIdentity>,
}

impl PinnedGitLayout<'_> {
    pub(crate) fn retain_directory(&mut self, pin: DirectoryIdentity) {
        self.pins.push(pin);
    }
}

impl GitRestoreFence for PinnedGitLayout<'_> {
    fn locations(&self) -> &[GitRestoreLocation] {
        &self.locations
    }
}

/// The complete protected named-child layout, without making any new
/// directories. The driver's original descriptor still owns child identity,
/// content and recovery checks. Retain these observations with the real
/// independent host fence. No containment or ambient authority.
pub(crate) fn pin_named_layout<'a>(
    lease: &'a File,
    root: &Path,
    expected_identity: (u64, u64),
    sources: &[GitSourceLocation],
    protected: &[&Path],
    allow_existing: bool,
) -> Result<PinnedGitLayout<'a>> {
    if sources.is_empty()
        || root.parent().is_none()
        || protected.iter().any(|path| overlaps(root, path))
    {
        return Err(Error::Denied(
            "Git maintenance requires a disjoint private named layout",
        ));
    }
    canonical_private_directory(root)?;
    let parent = DirectoryIdentity::open(root)?;
    let identity = parent.identity()?;
    if identity != expected_identity {
        return Err(Error::Conflict("approved named layout changed"));
    }
    for path in protected {
        if DirectoryIdentity::open(path)?.identity()? == identity {
            return Err(Error::Denied("approved named layout aliases host state"));
        }
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !allow_existing
            || !entry.file_type()?.is_dir()
            || !sources
                .iter()
                .any(|source| entry.file_name() == source.name.as_str())
        {
            return Err(Error::Conflict("named layout contains unapproved entries"));
        }
    }
    let mut pins = Vec::with_capacity(1 + sources.len() * 3);
    pins.push(parent);
    let mut exclusions = Vec::with_capacity(1 + protected.len());
    exclusions.extend_from_slice(protected);
    exclusions.push(root);
    let mut locations = Vec::with_capacity(sources.len());
    for source in sources {
        pins.extend(pin_private_checkout(source, &exclusions)?);
        locations.push(GitRestoreLocation {
            name: source.name.clone(),
            repository: source.repository.clone(),
            repository_identity: source.repository_identity,
            target: root.join(source.name.as_str()),
            parent_identity: identity,
        });
    }
    Ok(PinnedGitLayout {
        _lease: lease,
        locations,
        pins,
    })
}
