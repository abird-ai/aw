//! Host-independent Agent invariants. No transport, credentials, filesystem,
//! execution runtime or allocator is defined in this crate.
//! Standard Rust is preferred for hosted callers; core-only is opt-in.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod capability;

// Four portable runtime primitives and the two-verb hook vocabulary; the OS,
// scheduler, process and persistence services live in hosted adapters.
#[cfg(feature = "alloc")]
pub mod actor;
#[cfg(feature = "alloc")]
pub mod envelope;
#[cfg(feature = "alloc")]
pub mod hook_contract;
#[cfg(feature = "alloc")]
pub mod message;

/// Shared hosted/embedded turn authority; only compiled with alloc.
#[cfg(feature = "alloc")]
pub mod turn;
