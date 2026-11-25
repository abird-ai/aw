//! Explicit host-owned metadata decisions, renewed from a private file per
//! read. These are disclosure permissions, never source execution grants.
use crate::private_approval::{open_private_approval, overlaps};
use aw::{Error, PrincipalId, Result};
use aw_store::{SessionBinding, SessionStore, SourceMetadataApproval, SourceMetadataSpec};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    policy: SourceMetadataSpec,
}

pub(crate) struct ApprovalFile {
    path: PathBuf,
}

pub(crate) struct ApprovalRead {
    policy: SourceMetadataApproval,
    _lease: File,
}

impl ApprovalRead {
    pub(crate) fn policy(&self) -> &SourceMetadataApproval {
        &self.policy
    }
}

impl ApprovalFile {
    pub(crate) fn open(
        path: &Path,
        store: &SessionStore,
        actor: &PrincipalId,
        project: &str,
        binding: &SessionBinding,
    ) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or(Error::Denied("source metadata approval directory required"))?;
        let state = store
            .directory()
            .ok_or(Error::Denied("source metadata requires durable Store"))?;
        // The host independently protects policy/ancestors from all Agent
        // access and other writers. Reject observable overlap/aliases too.
        for protected in [state, &binding.tool_root, &binding.working_directory] {
            let canonical = protected.canonicalize()?;
            if overlaps(parent, &canonical)
                || aw_platform::DirectoryIdentity::open(parent)?.identity()?
                    == aw_platform::DirectoryIdentity::open(&canonical)?.identity()?
            {
                return Err(Error::Denied(
                    "source metadata policy overlaps protected execution or Store state",
                ));
            }
        }
        let file = Self {
            path: path.to_owned(),
        };
        file.read(store, actor, project)?;
        Ok(file)
    }

    pub(crate) fn read(
        &self,
        store: &SessionStore,
        actor: &PrincipalId,
        project: &str,
    ) -> Result<ApprovalRead> {
        let (lease, config): (_, Config) = open_private_approval(&self.path)?;
        let process = store.local_process_info()?;
        let spec = config.policy;
        if config.version != 1
            || store.directory() != Some(config.store_directory.as_path())
            || spec.store_id != store.store_id()?
            || spec.store_generation != process.store_generation
            || process.process_kind != "owner_lock"
            || process.state != "active"
            || &spec.actor != actor
            || spec.actor_user_id != store.current_user(actor)?.user_id
            || spec.project != project
        {
            return Err(Error::Denied(
                "source metadata approval differs from the current host subject",
            ));
        }
        Ok(ApprovalRead {
            policy: SourceMetadataApproval::new(spec)?,
            _lease: lease,
        })
    }
}
