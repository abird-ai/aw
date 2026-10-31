//! Reusable Unix filesystem authority: pinned root, no-follow traversal,
//! mutation locks, bounded staging and atomic publication.
//! aw-platform-unix reexports this module without changing its public API.
#[cfg(unix)]
pub mod scoped_fs;
