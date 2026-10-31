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
pub trait SecuredFilesystem: MetadataBackend {
    type Guard<'a>
    where
        Self: 'a;
    type Stage: StagedPublication<Error = Self::Error>;

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
