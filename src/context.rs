use crate::{Error, Result};
use aw_tools_adapter::process::ContainmentRequirement;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

/// Explicit trusted startup policy for the configured local native shell.
/// Capture once before state/executor setup; never infer it from a profile,
/// a Session, a model argument or a remote tool binding.
pub fn local_shell_containment_from_env() -> Result<ContainmentRequirement> {
    parse_local_shell_containment(std::env::var_os("AW_LOCAL_SHELL_CONTAINMENT").as_deref())
}

fn parse_local_shell_containment(value: Option<&OsStr>) -> Result<ContainmentRequirement> {
    match value {
        None => Ok(ContainmentRequirement::BestAvailable),
        Some(value) if value == "best-available" => Ok(ContainmentRequirement::BestAvailable),
        Some(value) if value == "closed-descendants" => {
            Ok(ContainmentRequirement::ClosedDescendants)
        }
        Some(_) => Err(Error::Config(
            "AW_LOCAL_SHELL_CONTAINMENT must be best-available or closed-descendants".into(),
        )),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_local_shell_minimum_is_exact_and_never_silently_downgraded() {
        assert_eq!(
            parse_local_shell_containment(None).unwrap(),
            ContainmentRequirement::BestAvailable
        );
        for (value, expected) in [
            ("best-available", ContainmentRequirement::BestAvailable),
            (
                "closed-descendants",
                ContainmentRequirement::ClosedDescendants,
            ),
        ] {
            assert_eq!(
                parse_local_shell_containment(Some(OsStr::new(value))).unwrap(),
                expected
            );
        }
        for value in ["", "closed", "ClosedDescendants", "closed-descendants "] {
            assert!(parse_local_shell_containment(Some(OsStr::new(value))).is_err());
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert!(parse_local_shell_containment(Some(OsStr::from_bytes(&[0xff]))).is_err());
        }
    }
}
