use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Errors intentionally exclude request bodies, bearer tokens and raw URLs.
#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration: {0}")]
    Config(String),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("trusted provider rejected before any request bytes were dispatched: {0}")]
    BeforeDispatch(&'static str),
    #[error("transport: {0}")]
    Transport(&'static str),
    #[error("provider returned HTTP {status}: {hint}; no automatic retry was made")]
    Http { status: u16, hint: &'static str },
    #[error("provider protocol: {0}")]
    Protocol(&'static str),
    #[error("{0}")]
    Limit(&'static str),
    #[error("request denied by extension: {0}")]
    Denied(&'static str),
    #[error("cancelled locally; the provider may still finish or bill this request")]
    Cancelled,
    #[error("session storage: {0}")]
    Storage(&'static str),
    #[error("session storage: {0}")]
    StorageDetail(String),
    #[error("session conflict: {0}")]
    Conflict(&'static str),
    #[error("request deadline exceeded; no automatic retry was made")]
    Timeout,
}

impl Error {
    pub fn http(status: u16) -> Self {
        let hint = match status {
            400 | 422 => "request rejected (check model, context length and endpoint options)",
            401 | 403 => {
                "authentication or access rejected; check the selected account/environment variable"
            }
            404 => "endpoint or model not found",
            408 | 504 => "server timed out",
            429 => "rate or quota limit reached",
            500..=599 => "server failure",
            300..=399 => "redirect refused; configure the final trusted endpoint explicitly",
            _ => "request was not accepted",
        };
        Self::Http { status, hint }
    }
}
