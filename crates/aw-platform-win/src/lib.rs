//! Windows-specific aw platform backend. The implementation is only compiled
//! for a Windows target; host-side workspace checks may still build this package.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
pub const FAMILY: &str = "windows";
