//! Shared native tool algorithms without a model, MCP transport, journal,
//! credential store, provider or ambient filesystem authority.
//! Synchronous operations are default; async, durable tasks, and sandboxing
//! are explicit host-selectable features. No implicit tool execution.
#[cfg(feature = "binary")]
pub mod binary;
pub mod filesystem;
#[cfg(feature = "write")]
pub mod matching;
#[cfg(feature = "binary")]
pub mod media;
#[cfg(feature = "write")]
pub mod mutation;
#[cfg(feature = "write")]
pub mod patch;
#[cfg(feature = "read")]
pub mod read;
#[cfg(feature = "sandbox")]
pub mod sandbox;
#[cfg(all(feature = "sandbox", target_os = "linux"))]
pub mod sandbox_linux;
#[cfg(feature = "read")]
pub mod search;
#[cfg(feature = "durable")]
pub mod task;
#[cfg(feature = "durable")]
pub mod task_checkpoint;
#[cfg(feature = "durable")]
pub mod task_fence;
#[cfg(feature = "durable")]
pub mod task_ops;
#[cfg(feature = "durable")]
pub mod task_retention;
#[cfg(feature = "durable")]
pub mod task_spool;
#[cfg(any(feature = "read", feature = "write"))]
pub mod text;

/// The host supplies pinned filesystem capabilities and exact grants.
pub trait ToolAuthority {
    type Error;
    fn check_read(&self, path: &str) -> Result<(), Self::Error>;
    fn check_write(&self, path: &str) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolEffect {
    Read,
    Write,
    Process,
    Unknown,
}

pub trait StopRequested {
    fn stopped(&self) -> bool;
}
impl StopRequested for () {
    fn stopped(&self) -> bool {
        false
    }
}
impl<F: Fn() -> bool> StopRequested for F {
    fn stopped(&self) -> bool {
        self()
    }
}

#[cfg(feature = "async")]
pub mod asynchronous {
    //! Optional offloading; never introduces a second durable task ledger.
    pub async fn run_blocking<T: Send + 'static>(
        operation: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, tokio::task::JoinError> {
        tokio::task::spawn_blocking(operation).await
    }
}
