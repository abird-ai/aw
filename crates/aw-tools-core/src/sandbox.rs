//! Host-approved sandbox policy validation. This does not launch a sandbox,
//! mint grants, or attest that namespaces/mounts/network filtering exist.
//! Actual isolation belongs to a platform host that pins and revalidates
//! directory handles at the moment it constructs its OS sandbox.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub const MAX_WRITE_ROOTS: usize = 16;
#[derive(Clone, Debug)]
pub struct SandboxPolicy<'a> {
    pub root: &'a Path,
    pub network: bool,
    pub writable_roots: &'a [&'a Path],
    /// Broad root write access must be an explicit host configuration.
    pub allow_root_write: bool,
}
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub(crate) struct PinnedSandboxDirectories {
    pub(crate) root: std::fs::File,
    pub(crate) writable: Vec<std::fs::File>,
}
#[derive(Clone, Debug)]
pub struct ValidatedSandboxPolicy {
    root: PathBuf,
    writable_roots: Vec<PathBuf>,
    network: bool,
    #[cfg(target_os = "linux")]
    pinned: std::sync::Arc<PinnedSandboxDirectories>,
}
impl ValidatedSandboxPolicy {
    #[cfg(target_os = "linux")]
    pub(crate) fn pinned_root(&self) -> &std::fs::File {
        &self.pinned.root
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn pinned_writes(&self) -> &[std::fs::File] {
        &self.pinned.writable
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn writable_roots(&self) -> &[PathBuf] {
        &self.writable_roots
    }
    pub fn network(&self) -> bool {
        self.network
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxPolicyError {
    InvalidRoot,
    InvalidPath,
    SymlinkOrReparsePoint,
    OutsideRoot,
    BroadRootWriteNotAuthorized,
    OverlappingWriteRoots,
    TooManyWriteRoots,
}
impl SandboxPolicy<'_> {
    /// Validate only structural policy; the OS executor must bind actual
    /// mount/network isolation and protect against concurrent path swaps.
    pub fn validate(&self) -> Result<ValidatedSandboxPolicy, SandboxPolicyError> {
        if self.writable_roots.len() > MAX_WRITE_ROOTS {
            return Err(SandboxPolicyError::TooManyWriteRoots);
        }
        let root = canonical_directory(self.root)?;
        let mut writes = Vec::with_capacity(self.writable_roots.len());
        for write in self.writable_roots {
            let path = canonical_directory(write)?;
            if !path.starts_with(&root) {
                return Err(SandboxPolicyError::OutsideRoot);
            }
            if path == root && !self.allow_root_write {
                return Err(SandboxPolicyError::BroadRootWriteNotAuthorized);
            }
            writes.push(path);
        }
        writes.sort();
        for pair in writes.windows(2) {
            if pair[1].starts_with(&pair[0]) {
                return Err(SandboxPolicyError::OverlappingWriteRoots);
            }
        }
        #[cfg(target_os = "linux")]
        let pinned = {
            // File identities are authorized and retained *here*, not
            // re-resolved from mutable path strings when the host launches.
            let root = crate::sandbox_linux::pin(&root, true)
                .map_err(|_| SandboxPolicyError::InvalidRoot)?;
            let mut writable = Vec::with_capacity(writes.len());
            for path in &writes {
                writable.push(
                    crate::sandbox_linux::pin(path, true)
                        .map_err(|_| SandboxPolicyError::InvalidPath)?,
                );
            }
            std::sync::Arc::new(PinnedSandboxDirectories { root, writable })
        };
        Ok(ValidatedSandboxPolicy {
            root,
            writable_roots: writes,
            network: self.network,
            #[cfg(target_os = "linux")]
            pinned,
        })
    }
}
fn canonical_directory(path: &Path) -> Result<PathBuf, SandboxPolicyError> {
    if !path.is_absolute() {
        return Err(SandboxPolicyError::InvalidPath);
    }
    if path
        .components()
        .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(SandboxPolicyError::InvalidPath);
    }
    // Symlink/junction components are forbidden even if their canonical
    // destination lies within the root: don't silently turn aliases into
    // different mount or write permissions.
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        let metadata =
            fs::symlink_metadata(&prefix).map_err(|_| SandboxPolicyError::InvalidPath)?;
        if metadata.file_type().is_symlink() || reparse(&metadata) {
            return Err(SandboxPolicyError::SymlinkOrReparsePoint);
        }
        if !metadata.is_dir() {
            return Err(SandboxPolicyError::InvalidPath);
        }
    }
    path.canonicalize()
        .map_err(|_| SandboxPolicyError::InvalidPath)
}
#[cfg(windows)]
fn reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn reparse(_metadata: &fs::Metadata) -> bool {
    false
}

/// The host must accept only validated policies and recheck with pinned
/// capabilities immediately before launch. A successfully validated path is
/// not itself a sandbox or evidence of network isolation.
pub trait SandboxHost {
    type Error;
    type Prepared;
    fn prepare(&self, config: &ValidatedSandboxPolicy) -> Result<Self::Prepared, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "abird-sandbox-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn rejects_implicit_root_write_and_overlapping_permissions() {
        let root = Temp::new();
        let child = root.0.join("src");
        let nested = child.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let root_policy = SandboxPolicy {
            root: &root.0,
            network: false,
            writable_roots: &[root.0.as_path()],
            allow_root_write: false,
        };
        assert_eq!(
            root_policy.validate().unwrap_err(),
            SandboxPolicyError::BroadRootWriteNotAuthorized
        );
        let overlap = SandboxPolicy {
            root: &root.0,
            network: false,
            writable_roots: &[child.as_path(), nested.as_path()],
            allow_root_write: false,
        };
        assert_eq!(
            overlap.validate().unwrap_err(),
            SandboxPolicyError::OverlappingWriteRoots
        );
        let valid = SandboxPolicy {
            root: &root.0,
            network: false,
            writable_roots: &[nested.as_path()],
            allow_root_write: false,
        };
        let validated = valid.validate().unwrap();
        assert_eq!(validated.writable_roots(), &[nested]);
        assert!(!validated.network());
    }
    #[cfg(unix)]
    #[test]
    fn denies_symlink_components_and_parent_traversal() {
        let root = Temp::new();
        let actual = root.0.join("real");
        let shortcut = root.0.join("alias");
        fs::create_dir_all(&actual).unwrap();
        symlink(&actual, &shortcut).unwrap();
        let policy = SandboxPolicy {
            root: &root.0,
            network: true,
            writable_roots: &[shortcut.as_path()],
            allow_root_write: false,
        };
        assert_eq!(
            policy.validate().unwrap_err(),
            SandboxPolicyError::SymlinkOrReparsePoint
        );
        let traversal = root.0.join("real/../real");
        let bad = SandboxPolicy {
            root: &root.0,
            network: true,
            writable_roots: &[traversal.as_path()],
            allow_root_write: false,
        };
        assert_eq!(bad.validate().unwrap_err(), SandboxPolicyError::InvalidPath);
    }
    #[test]
    fn outside_root_and_relative_policy_cannot_be_installed() {
        let root = Temp::new();
        let other = Temp::new();
        let denied = SandboxPolicy {
            root: &root.0,
            network: false,
            writable_roots: &[other.0.as_path()],
            allow_root_write: false,
        };
        assert_eq!(
            denied.validate().unwrap_err(),
            SandboxPolicyError::OutsideRoot
        );
        let relative = SandboxPolicy {
            root: Path::new("relative"),
            network: false,
            writable_roots: &[],
            allow_root_write: false,
        };
        assert_eq!(
            relative.validate().unwrap_err(),
            SandboxPolicyError::InvalidPath
        );
    }
}
