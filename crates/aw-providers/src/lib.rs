//! Explicit HTTP adapters for [`aw_agent_core`]. No CLI, web server or terminal
//! dependencies. Core-only consumers need not depend on this crate.
pub mod catalog;
pub mod config;
pub mod discovery;
pub mod provider;
pub use aw_agent_core::{Error, Message, Result};
pub use provider::{HttpProvider, LazyHttpProvider};

fn transport(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Timeout
    } else if error.is_connect() {
        Error::Transport("connection failed (check endpoint, TLS trust and server availability)")
    } else {
        Error::Transport("request or response stream failed")
    }
}
