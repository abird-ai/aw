//! Shared bounded, private local host policy input and live advisory lease.
use aw::{Error, Result};
use fs2::FileExt;
use serde::de::DeserializeOwned;
use std::{fs::File, io::Read, path::Path};

pub(crate) fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

pub(crate) fn canonical_private_directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.canonicalize()? != path {
        return Err(Error::Denied(
            "host approval requires canonical private directories",
        ));
    }
    aw_platform::validate_private_dir(path)?;
    Ok(())
}

/// Cooperating host policy writers lock this same inode before
/// editing/withdrawing. No replacement while leased; this is not a
/// physical isolation or external-writer guarantee.
pub(crate) fn lock_private_approval(path: &Path) -> Result<File> {
    let parent = path
        .parent()
        .ok_or(Error::Denied("host approval directory required"))?;
    canonical_private_directory(parent)?;
    if !path.is_absolute() || path.canonicalize()? != path {
        return Err(Error::Denied("host approval requires a canonical file"));
    }
    let file = aw_platform::open_private_existing(path, true)?;
    FileExt::try_lock_exclusive(&file).map_err(|_| Error::Conflict("host approval is busy"))?;
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
        return Err(Error::Limit("host approval exceeds 1 MiB"));
    }
    let config = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Config("invalid host approval".into()))?;
    Ok((file, config))
}
