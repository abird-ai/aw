//! One capability-backed filesystem contract for AW and future dotlink.
//! OS-specific implementations keep real descriptor/handle authority and
//! lock/staging semantics; neither model adapter may bypass this interface.
#[cfg(feature = "rw")]
use super::lifecycle::RemoveOutcome;
use super::{walk, EntryMeta};
#[cfg(feature = "rw")]
use crate::read::{Page, PageRequest};

pub trait MetadataBackend: walk::Backend {
    /// Name-only lstat: never follows links or materializes host paths.
    fn entry_meta(&self, path: &str) -> Result<EntryMeta, Self::Error>;
}

/// Resolve only an explicitly requested final content symlink. Intermediate
/// components remain under the backend's ordinary no-follow rules. Resolution
/// itself is metadata-only; the host must re-authorize a changed returned path
/// before any payload read or mutation.
pub trait ExplicitPathBackend: MetadataBackend {
    /// `allow_missing` is for create-or-replace content tools. A dangling final
    /// symlink is never considered missing and must fail closed.
    fn resolve_content_path(&self, path: &str, allow_missing: bool) -> Result<String, Self::Error>;
    /// Resolve only an explicitly requested final directory symlink. Child
    /// traversal remains under the backend's ordinary no-follow rules.
    fn resolve_directory_path(&self, path: &str) -> Result<String, Self::Error>;
}
/// A staged update is not visible until explicitly committed by the same
/// mutation owner that reserved the source/destination names.
#[cfg(feature = "rw")]
pub trait StagedPublication {
    type Error;
    fn commit(self) -> Result<(), Self::Error>;
}
/// A native operation may fail either before its one visible
/// publication, or at an indeterminate commit boundary. Do not infer
/// a safe retry merely from the OS error string.
#[cfg(feature = "rw")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitFailure {
    NoPublication,
    Unknown,
}
#[cfg(feature = "rw")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationAccess {
    Read,
    Write,
    ReadWrite,
}
#[cfg(feature = "rw")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationExtent {
    Exact,
    Subtree,
}
#[cfg(feature = "rw")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MutationPath<'a> {
    pub path: &'a str,
    pub access: MutationAccess,
    pub extent: MutationExtent,
}

#[cfg(feature = "rw")]
pub trait SecuredFilesystem: MetadataBackend {
    type Guard<'a>
    where
        Self: 'a;
    type Stage: StagedPublication<Error = Self::Error>;

    /// Authority-only preflight before any mutation lock is acquired. This is
    /// a zero-cost no-op for a single rooted capability; multi-root hosts use it
    /// to reject read/write exclusion holes without traversing them first.
    fn preflight_mutation(&self, _paths: &[MutationPath<'_>]) -> Result<(), Self::Error> {
        Ok(())
    }

    /// No-follow metadata needed only for mutation planning. A host with
    /// asymmetric grants may authorize this through write authority without
    /// granting payload read access or an equality oracle. Single-root
    /// backends default to the ordinary metadata path.
    fn mutation_meta(&self, path: &str) -> Result<EntryMeta, Self::Error> {
        self.entry_meta(path)
    }

    /// Stable host-namespace key used only to detect two model spellings that
    /// name the same mutation entry before planning. This is not object/inode
    /// identity: distinct hard-link names remain distinct namespace entries.
    fn mutation_path_key(&self, path: &str) -> Result<Vec<u8>, Self::Error> {
        Ok(path.as_bytes().to_vec())
    }

    /// Existing root-wide/cross-process mutation admission, not a new lock.
    fn lock<'a>(
        &'a self,
        paths: &[String],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Guard<'a>, Self::Error>;
    fn stage_create(
        &self,
        path: &str,
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage, Self::Error>;
    fn stage_replace(
        &self,
        path: &str,
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage, Self::Error>;
    fn stage_update(
        &self,
        path: &str,
        expected: &[u8],
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage, Self::Error>;
    /// Stage-commit and exact copy failure classification. A post-publication
    /// cleanup error must never be marked NoPublication.
    fn classify_publication_failure(&self, error: &Self::Error) -> CommitFailure;
    fn copy_exact(
        &self,
        source: &str,
        destination: &str,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(), Self::Error>;
    fn move_exact(
        &self,
        source: &str,
        destination: &str,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(), Self::Error>;
    fn remove_exact(
        &self,
        paths: &[String],
        recursive: bool,
        cancel: &dyn Fn() -> bool,
    ) -> Result<RemoveOutcome, Self::Error>;
    fn read_page(
        &self,
        path: &str,
        request: PageRequest,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Page, Self::Error>;
}
