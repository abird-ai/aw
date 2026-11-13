//! Backend-independent Session access and coordinator-binding contracts.
//! Durable membership, history and writer claims belong to the actual backend.

#![forbid(unsafe_code)]

use aw_agent_core::{Error, PrincipalId, Result};
use aw_agent_kernel::WriterFence;
use aw_store_api::{BranchInfo, SessionBackend, SessionBinding, SessionCursor, SessionInfo};
use std::{num::NonZeroU64, sync::Arc};

/// Host-scoped access. The backend authenticates each operation under this
/// original principal; constructing this view does not issue any grants.
pub struct Sessions<'a, B: SessionBackend + ?Sized> {
    backend: &'a B,
    actor: &'a PrincipalId,
}

impl<'a, B: SessionBackend + ?Sized> Sessions<'a, B> {
    pub fn new(backend: &'a B, actor: &'a PrincipalId) -> Self {
        Self { backend, actor }
    }

    pub fn create(
        &self,
        title: &str,
        binding: SessionBinding,
    ) -> Result<(SessionInfo, BranchInfo, Arc<dyn SessionCursor>)> {
        self.backend.create(self.actor, title, binding)
    }

    pub fn open(&self, session: &str, branch: &str) -> Result<Arc<dyn SessionCursor>> {
        self.backend.open(self.actor, session, branch)
    }

    pub fn list(&self) -> Result<Vec<SessionInfo>> {
        self.backend.list(self.actor)
    }
}

/// An immutable, host-verified coordination claim, not a bearer capability.
/// SessionStore must persist uniqueness/epochs before any paid Turn admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinatorBinding {
    session_id: String,
    branch_id: String,
    owner: PrincipalId,
    agent_ref: String,
    binding_revision: NonZeroU64,
    agent_writer_fence: WriterFence,
    workspace_writer_fence: WriterFence,
}

impl CoordinatorBinding {
    /// The host must canonicalize and authorize agent_ref before calling.
    pub fn from_trusted_host(
        session: &SessionInfo,
        branch: &BranchInfo,
        agent_ref: impl Into<String>,
        binding_revision: NonZeroU64,
        agent_writer_fence: WriterFence,
        workspace_writer_fence: WriterFence,
    ) -> Result<Self> {
        let agent_ref = agent_ref.into();
        if session.id != branch.session_id
            || agent_ref.is_empty()
            || agent_ref.len() > 1024
            || agent_ref.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid Session coordinator binding"));
        }
        Ok(Self {
            session_id: session.id.clone(),
            branch_id: branch.id.clone(),
            owner: session.owner.clone(),
            agent_ref,
            binding_revision,
            agent_writer_fence,
            workspace_writer_fence,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn branch_id(&self) -> &str {
        &self.branch_id
    }

    pub fn agent_ref(&self) -> &str {
        &self.agent_ref
    }

    /// Compare every identity/revision, including coordinator replacement.
    /// The real store must still check current owner and writer authority.
    pub fn matches(&self, current: &Self) -> bool {
        self == current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn session() -> SessionInfo {
        SessionInfo {
            id: "s1".into(),
            owner: PrincipalId::new("host:local").unwrap(),
            title: "Private".into(),
            imported: false,
            workspace_id: String::new(),
            binding: SessionBinding {
                working_directory: PathBuf::from("."),
                tool_root: PathBuf::from("."),
                system_prompt: String::new(),
                profile_hint: String::new(),
                model_hint: String::new(),
                destination_fingerprint: String::new(),
            },
            root_node: "n1".into(),
            next_turn: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn coordinator_binding_fences_both_writers() {
        let session = session();
        let branch = BranchInfo {
            id: "b1".into(),
            session_id: "s1".into(),
            name: "main".into(),
            head: "n1".into(),
            revision: 1,
            active_run: None,
        };
        let nonzero = |value| NonZeroU64::new(value).unwrap();
        let make =
            |agent: &str, binding_revision: u64, writer: Option<u64>, workspace: Option<u64>| {
                CoordinatorBinding::from_trusted_host(
                    &session,
                    &branch,
                    agent,
                    nonzero(binding_revision),
                    writer.map_or(WriterFence::NotApplicable, |value| {
                        WriterFence::epoch(nonzero(value))
                    }),
                    workspace.map_or(WriterFence::NotApplicable, |value| {
                        WriterFence::epoch(nonzero(value))
                    }),
                )
                .unwrap()
            };
        let binding = make("/d:0/i:0/p:0/a:0", 1, Some(2), Some(7));
        assert!(binding.matches(&make("/d:0/i:0/p:0/a:0", 1, Some(2), Some(7))));
        assert!(!binding.matches(&make("/d:0/i:0/p:0/a:0", 1, Some(2), Some(8))));
        assert!(!binding.matches(&make("/d:0/i:0/p:0/a:0", 2, Some(2), Some(7))));
        assert!(!binding.matches(&make("/d:0/i:0/p:0/a:2", 1, Some(2), Some(7))));
        let personal = make("/d:0/i:0/p:0/a:0", 1, None, None);
        assert!(personal.matches(&make("/d:0/i:0/p:0/a:0", 1, None, None)));
        assert!(!binding.matches(&personal));
    }
}
