//! Shared local-host observations and approval lease for offline Git
//! maintenance. Pins and advisory locks are not physical containment.
use aw::{Error, Result};
use aw_git::source_driver::GitSourceLocation;
use aw_platform::DirectoryIdentity;
use aw_store::SessionStore;
use fs2::FileExt;
use serde::de::DeserializeOwned;
use std::{fs::File, io::Read, path::Path};

pub(crate) fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

pub(crate) fn canonical_private_directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.canonicalize()? != path {
        return Err(Error::Denied(
            "Git maintenance requires canonical private directories",
        ));
    }
    aw_platform::validate_private_dir(path)?;
    Ok(())
}

/// The operator's cooperating config writers lock this same inode before
/// editing/withdrawing. No replacement while leased; the operator also owns
/// physical exclusion of all Git writers and other retention users.
pub(crate) fn lock_private_approval(path: &Path) -> Result<File> {
    let parent = path
        .parent()
        .ok_or(Error::Denied("Git approval directory required"))?;
    canonical_private_directory(parent)?;
    if !path.is_absolute() || path.canonicalize()? != path {
        return Err(Error::Denied("Git approval requires a canonical file"));
    }
    let file = aw_platform::open_private_existing(path, true)?;
    FileExt::try_lock_exclusive(&file).map_err(|_| Error::Conflict("Git approval is busy"))?;
    Ok(file)
}

/// One bounded, strict typed approval document under the same live lease.
pub(crate) fn open_private_approval<T: DeserializeOwned>(path: &Path) -> Result<(File, T)> {
    const MAX_APPROVAL_BYTES: usize = 1024 * 1024;
    let file = lock_private_approval(path)?;
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_APPROVAL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_APPROVAL_BYTES {
        return Err(Error::Limit("Git approval exceeds 1 MiB"));
    }
    let config = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Config("invalid Git maintenance approval".into()))?;
    Ok((file, config))
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
