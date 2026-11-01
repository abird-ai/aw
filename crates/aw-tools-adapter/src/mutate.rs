//! Shared file replacement contract for text/binary tools.
//! The platform layer owns locking and staged publication; no other writer
//! bypasses its preimage checks or invents a separate commit path.
use crate::platform_fs;
use aw_agent_core::{Error, Result};
use aw_platform::scoped_fs::{Root, ScopedError};
use sha2::{Digest, Sha256};
use std::io;

const MAX_PREIMAGE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn validate_digest(value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Denied(
                "expected_sha256 must be 64 hexadecimal characters",
            ));
        }
    }
    Ok(())
}

/// Returns whether bytes were actually published. A no-op preserves inode,
/// metadata and mtime. An explicit digest additionally pins the preimage.
pub(crate) fn write(
    root: &Root,
    path: &str,
    bytes: &[u8],
    create_only: bool,
    expected_sha256: Option<&str>,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool> {
    validate_digest(expected_sha256)?;
    let _guard = platform_fs(root.lock_mutations_with_cancel(&[path.to_owned()], cancelled))?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let original = match root.read_bytes(path, MAX_PREIMAGE_BYTES) {
        Ok(data) => Some(data),
        Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => None,
        Err(ScopedError::Limit(_)) if expected_sha256.is_none() && !create_only => {
            // The original is too large to buffer. A plain explicitly granted
            // replacement may still succeed, but a conditional write never
            // discards a preimage it cannot verify.
            let staged = platform_fs(root.stage_replace(path, bytes, cancelled))?;
            if cancelled() {
                return Err(Error::Cancelled);
            }
            return platform_fs(staged.commit()).map(|()| true);
        }
        Err(error) => return platform_fs(Err(error)),
    };
    if create_only && original.is_some() {
        return Err(Error::Denied("create_only refuses an existing target"));
    }
    if let Some(expected) = expected_sha256 {
        let Some(original) = original.as_ref() else {
            return Err(Error::Denied("expected_sha256 requires an existing file"));
        };
        if format!("{:x}", Sha256::digest(original)) != expected.to_ascii_lowercase() {
            return Err(Error::Denied("expected_sha256 precondition mismatch"));
        }
    }
    if original.as_deref() == Some(bytes) {
        return Ok(false);
    }
    let staged = match original.as_deref() {
        Some(old) => root.stage_update(path, old, bytes, cancelled),
        None => root.stage_create(path, bytes, cancelled),
    };
    let staged = platform_fs(staged)?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    platform_fs(staged.commit())?;
    Ok(true)
}
