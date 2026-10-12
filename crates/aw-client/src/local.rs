//! Same-user local-owner endpoint boundary. OS-specific pathname/socket
//! validation and listener cleanup live in aw-platform-*.
use crate::{Error, Result};
use std::path::Path;

pub fn effective_uid() -> Result<u32> {
    aw_platform::effective_uid().ok_or(Error::Unsupported)
}

pub fn private_parent(path: &Path, uid: u32) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        aw_platform::local_ipc_private_parent(path, uid).map_err(Error::Io)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, uid);
        Err(Error::Unsupported)
    }
}

pub fn socket_identity(path: &Path, uid: u32) -> Result<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        aw_platform::local_ipc_socket_identity(path, uid).map_err(Error::Io)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, uid);
        Err(Error::Unsupported)
    }
}

#[cfg(target_os = "linux")]
pub type BoundSocket = aw_platform::LocalBoundSocket;
