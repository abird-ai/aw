//! Minimal host-driven embedded Agent adapter. The authoritative turn state
//! is aw-agent-kernel::turn::TurnMachine, used by the real hosted Agent too.
//! This adapter performs no I/O until a host explicitly starts or steps it.
//! It does not own a model/network stack, clock, RNG, store, scheduler or tools.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
mod engine;
#[cfg(feature = "alloc")]
pub use engine::*;

#[cfg(not(feature = "alloc"))]
compile_error!("aw-embedded requires the alloc feature; use aw-agent-kernel for core-only.");
