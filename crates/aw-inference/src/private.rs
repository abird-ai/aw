//! Platform-verified owner-private files. This does not isolate hostile
//! same-principal native code; platform-specific ACL/mode checks live in
//! aw-platform-* rather than this inference crate.
use crate::{Error, Result};
use std::{
    io::{Read, Write},
    path::{Component, Path},
};

pub fn parent(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(Error::Config("use an absolute private file path".into()));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::CurDir | Component::Prefix(_)
        )
    }) || path.to_string_lossy().chars().any(char::is_control)
    {
        return Err(Error::Denied("unsafe private path"));
    }
    let parent = path
        .parent()
        .ok_or(Error::Denied("missing private parent"))?;
    aw_platform::validate_private_dir(parent)
        .map_err(|_| Error::Denied("private parent requires platform-verified owner-only custody"))
}

pub fn read(path: &Path, max: usize) -> Result<Vec<u8>> {
    parent(path)?;
    let mut file = match aw_platform::open_private_existing(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(error.into()),
        Err(_) => {
            return Err(Error::Denied(
                "private file requires platform-verified owner-only custody",
            ))
        }
    };
    let mut bytes = Vec::new();
    (&mut file).take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(Error::Limit("private configuration exceeds limit"));
    }
    Ok(bytes)
}

pub fn create(path: &Path, bytes: &[u8]) -> Result<()> {
    parent(path)?;
    let mut file = aw_platform::open_private_new(path).map_err(|_| {
        Error::Denied("private file creation requires platform-verified owner-only custody")
    })?;
    file.write_all(bytes)?;
    file.sync_all()?;
    aw_platform::sync_parent(path)?;
    Ok(())
}

pub fn key(path: &Path) -> Result<crate::protocol::PairingKey> {
    let bytes = read(path, 32)?;
    let key: [u8; 32] = bytes.try_into().map_err(|_| {
        Error::Config(
            "pairing capability must be exactly 32 random bytes; not a model API key".into(),
        )
    })?;
    Ok(crate::protocol::PairingKey::from_bytes(key))
}
