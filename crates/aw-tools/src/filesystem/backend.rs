//! One capability-backed filesystem contract for AW and future dotlink.
//! OS-specific implementations keep real descriptor/handle authority and
//! lock/staging semantics; neither model adapter may bypass this interface.
#[cfg(feature = "rw")]
use super::lifecycle::RemoveOutcome;
use super::{walk, EntryMeta};
#[cfg(feature = "rw")]
use crate::read::{Page, PageRequest};
#[cfg(feature = "rw")]
use std::fs::File;

pub trait MetadataBackend: walk::Backend {
    /// Name-only lstat: never follows links or materializes host paths.
    fn entry_meta(&self, path: &str) -> Result<EntryMeta, Self::Error>;

    /// Conservative scheduler-only namespace spelling. This never grants or
    /// resolves authority: it only lets a platform map namespace aliases (for
    /// example Windows case variants) to one hierarchical conflict key.
    /// Backends whose namespace is byte/case exact may keep the model spelling.
    fn scheduling_path(&self, path: &str) -> Result<String, Self::Error> {
        Ok(path.to_owned())
    }
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
#[derive(Debug)]
pub enum StreamWriteError {
    Io(std::io::Error),
    Cancelled,
    Refused(&'static str),
}
#[cfg(feature = "rw")]
impl From<std::io::Error> for StreamWriteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
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
#[derive(Clone, Copy, Debug)]
pub struct CreateFile<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
}

#[cfg(feature = "rw")]
pub struct CreateStage<S> {
    /// Earliest input contributing to this atomic namespace publication unit.
    pub first: usize,
    /// All input indices that become visible with this one publication.
    pub inputs: Vec<usize>,
    pub stage: S,
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
    /// Stage ordered create-only files. Backends coalesce files that share one
    /// not-yet-visible publication root and return units carrying every
    /// contributing input index in deterministic earliest-contributor order.
    fn stage_create_many(
        &self,
        files: &[CreateFile<'_>],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<CreateStage<Self::Stage>>, Self::Error>;
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
    /// Open an existing regular mutation source under read+write authority.
    /// The returned handle pins the source object for streamed planning.
    fn open_regular_for_update(&self, path: &str) -> Result<File, Self::Error>;
    /// Stage a digest-verified streamed replacement. Writer errors occur before
    /// public namespace publication and are mapped by the backend accordingly.
    fn stage_stream_update(
        &self,
        path: &str,
        expected: (u64, [u8; 32]),
        writer: &mut dyn FnMut(&mut File) -> Result<(), StreamWriteError>,
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
