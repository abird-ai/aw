//! Target-selected platform facade. Higher-level aw crates depend on this crate,
//! never directly on libc/Win32 for equivalent operating-system primitives.

#[cfg(unix)]
pub use aw_platform_unix::*;
#[cfg(windows)]
pub use aw_platform_win::*;

#[cfg(not(any(unix, windows)))]
compile_error!("aw currently supports Unix-family and Windows targets");
