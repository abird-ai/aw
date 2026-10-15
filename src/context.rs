use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// Fixed execution context. Capturing a directory does not read project files or
/// send its path/contents to a provider. Never change process-global cwd.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub working_directory: PathBuf,
    pub effective_uid: Option<u32>,
}

impl ExecutionContext {
    pub fn capture(directory: &Path) -> Result<Self> {
        let working_directory = directory.canonicalize()?;
        if !working_directory.is_dir() {
            return Err(Error::Config("-C must name a directory".into()));
        }
        let effective_uid = aw_platform::effective_uid();
        Ok(Self {
            working_directory,
            effective_uid,
        })
    }
}
