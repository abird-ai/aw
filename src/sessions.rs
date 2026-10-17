//! Application-owned durable session composition. Neither view nor stored data
//! constructs credentials, grants or provider clients. The current host does so.
use crate::{
    config::{Profile, Settings},
    context::ExecutionContext,
    LocalToolOptions,
};
use aw_agent_core::{Agent, Error, PrincipalId, Result};
use aw_store::{
    BranchInfo, ContextInfo, SessionBinding, SessionHandle, SessionInfo, SessionStore, SessionView,
};
#[cfg(feature = "jsonl")]
use aw_store_api::{SessionBackend, SessionCursor};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn default_directory() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("AW_STATE_DIR") {
        return Ok(PathBuf::from(p));
    }
    let base = if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(p)
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
            Error::Config(
                "HOME/XDG_STATE_HOME missing; set --state-dir or --state-memory-only".into(),
            )
        })?)
        .join(".local/state")
    };
    Ok(base.join("abird/aw"))
}

pub fn actor(_context: &ExecutionContext, web: bool) -> Result<PrincipalId> {
    if web {
        PrincipalId::new("browser:paired")
    } else {
        PrincipalId::new(aw_platform::local_principal()?)
    }
}

#[cfg(feature = "jsonl")]
pub fn jsonl_store(directory: &Path) -> Result<aw_store_jsonl::JsonlStore> {
    if aw_platform::secure_private_files_supported() {
        aw_platform::ensure_private_dir(directory)
            .map_err(|_| Error::Denied("JSONL state directory is not private"))?;
    } else if !directory.exists() {
        std::fs::create_dir_all(directory)?;
    }
    aw_store_jsonl::JsonlStore::open(directory.join("sessions-jsonl"))
}
pub fn destination_fingerprint(profile: &Profile) -> Result<String> {
    crate::config::destination_fingerprint(profile)
}

pub fn binding(
    settings: &Settings,
    context: &ExecutionContext,
    tools: &LocalToolOptions,
) -> Result<SessionBinding> {
    let profile = &settings.profile;
    let fingerprint = destination_fingerprint(profile)?;
    Ok(SessionBinding {
        working_directory: context.working_directory.clone(),
        tool_root: tools.root.canonicalize()?,
        system_prompt: settings.system_prompt.clone(),
        profile_hint: settings.profile_name.clone(),
        model_hint: profile
            .model
            .clone()
            .ok_or_else(|| Error::Config("model required before creating a session".into()))?,
        destination_fingerprint: fingerprint,
    })
}

#[derive(Clone)]
pub struct PersistentSession {
    pub store: SessionStore,
    pub actor: PrincipalId,
    pub handle: SessionHandle,
    pub binding: SessionBinding,
    pub allow_transfer: bool,
}
impl PersistentSession {
    pub fn create(
        store: SessionStore,
        actor: PrincipalId,
        binding: SessionBinding,
        title: &str,
    ) -> Result<Self> {
        let (s, b) = store.create(&actor, title, binding.clone())?;
        let (_, handle) = store.activate(&actor, &s.id, &b.id)?;
        Ok(Self {
            store,
            actor,
            handle,
            binding,
            allow_transfer: false,
        })
    }
    pub fn create_in_workspace(
        store: SessionStore,
        actor: PrincipalId,
        binding: SessionBinding,
        title: &str,
        domain: Option<&str>,
        workspace: Option<&str>,
        target: Option<aw_store::SessionExecutionTarget>,
    ) -> Result<Self> {
        let (s, b) =
            store.create_in_workspace(&actor, title, binding.clone(), domain, workspace, target)?;
        let (_, handle) = store.activate(&actor, &s.id, &b.id)?;
        Ok(Self {
            store,
            actor,
            handle,
            binding,
            allow_transfer: false,
        })
    }
    pub fn open(
        store: SessionStore,
        actor: PrincipalId,
        binding: SessionBinding,
        session: &str,
        branch: &str,
        allow_transfer: bool,
    ) -> Result<Self> {
        let (view, handle) = store.activate(&actor, session, branch)?;
        check_binding(&binding, &view.session.binding, allow_transfer)?;
        if !matches!(
            store.session_target(&actor, session)?,
            aw_store::SessionExecutionTarget::Local { .. }
        ) {
            return Err(Error::Denied("this Session targets a remote Machine/Fleet; Phase 13 has no QUIC execution dispatcher yet"));
        }
        Ok(Self {
            store,
            actor,
            handle,
            binding,
            allow_transfer,
        })
    }
    pub fn view(&self) -> Result<SessionView> {
        self.handle.view()
    }
    pub fn apply(&self, agent: &mut Agent) -> Result<()> {
        let view = self.view()?;
        agent.load_history(view.history, Some(Arc::new(self.handle.clone())))
    }
    pub fn new_session(&self, title: &str) -> Result<Self> {
        let info = self.store.session(&self.actor, self.handle.session_id())?;
        let target = self
            .store
            .session_target(&self.actor, self.handle.session_id())?;
        let (s, b) = self.store.create_in_workspace(
            &self.actor,
            title,
            self.binding.clone(),
            None,
            Some(&info.workspace_id),
            Some(target),
        )?;
        let (_, handle) = self.store.activate(&self.actor, &s.id, &b.id)?;
        Ok(Self {
            store: self.store.clone(),
            actor: self.actor.clone(),
            handle,
            binding: self.binding.clone(),
            allow_transfer: false,
        })
    }
    pub fn open_session(&self, session: &str, branch: &str) -> Result<Self> {
        Self::open(
            self.store.clone(),
            self.actor.clone(),
            self.binding.clone(),
            session,
            branch,
            self.allow_transfer && session == self.handle.session_id(),
        )
    }
    /// Reactivate an already-saved Session using its own durable binding.
    ///
    /// This is intentionally separate from `open_session`, which validates that
    /// the caller is not trying to move a Session into the caller's current
    /// directory/tool root/destination. Native workspace selection uses this
    /// method and then reconstructs the Host from the returned saved binding.
    pub fn open_saved(
        store: SessionStore,
        actor: PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<Self> {
        let (view, handle) = store.activate(&actor, session, branch)?;
        if !matches!(
            store.session_target(&actor, session)?,
            aw_store::SessionExecutionTarget::Local { .. }
        ) {
            return Err(Error::Denied(
                "this Session targets a remote Machine/Fleet; local workspace selection cannot dispatch it",
            ));
        }
        Ok(Self {
            store,
            actor,
            handle,
            binding: view.session.binding,
            allow_transfer: false,
        })
    }

    pub fn open_saved_session(&self, session: &str, branch: &str) -> Result<Self> {
        Self::open_saved(self.store.clone(), self.actor.clone(), session, branch)
    }

    /// Explicit disclosure transition used by reviewed shared-Session flows.
    ///
    /// This preserves root/system-instruction equality while allowing the
    /// destination/account fingerprint to differ. Calling this method is the
    /// explicit context-transfer decision; ordinary Session open never gains
    /// this behavior implicitly.
    pub fn open_session_with_context_transfer(&self, session: &str, branch: &str) -> Result<Self> {
        Self::open(
            self.store.clone(),
            self.actor.clone(),
            self.binding.clone(),
            session,
            branch,
            true,
        )
    }
    pub fn switch_branch(&self, branch: &str) -> Result<Self> {
        self.open_session(self.handle.session_id(), branch)
    }
    pub fn fork(&self, name: &str, at: Option<&str>) -> Result<Self> {
        let b = self.handle.branch()?;
        let next = self.store.fork(
            &self.actor,
            self.handle.session_id(),
            name,
            at.unwrap_or(&b.head),
        )?;
        self.switch_branch(&next.id)
    }
    pub fn checkpoint(&self, summary: &str) -> Result<Self> {
        let b = self.handle.branch()?;
        let next = self.store.checkpoint(
            &self.actor,
            self.handle.session_id(),
            &b.id,
            b.revision,
            summary,
        )?;
        self.switch_branch(&next.id)
    }
    pub fn list(&self) -> Result<Vec<SessionInfo>> {
        self.store.list(&self.actor)
    }
    pub fn set_title(&self, title: &str) -> Result<SessionInfo> {
        self.store
            .set_title(&self.actor, self.handle.session_id(), title)
    }
    pub fn branches(&self) -> Result<Vec<BranchInfo>> {
        self.store.branches(&self.actor, self.handle.session_id())
    }
    pub fn context(&self) -> Result<ContextInfo> {
        self.store.context_info(
            &self.actor,
            self.handle.session_id(),
            &self.handle.branch()?.id,
        )
    }
    pub fn export(&self, path: &Path) -> Result<()> {
        self.store
            .export_file(&self.actor, self.handle.session_id(), path)
    }
}
#[cfg(feature = "jsonl")]
#[derive(Clone)]
pub struct JsonlPersistentSession {
    pub store: aw_store_jsonl::JsonlStore,
    pub actor: PrincipalId,
    pub cursor: Arc<dyn SessionCursor>,
    pub binding: SessionBinding,
    pub allow_transfer: bool,
}

#[cfg(feature = "jsonl")]
pub fn open_jsonl_store(state_dir: &Path) -> Result<aw_store_jsonl::JsonlStore> {
    if aw_platform::secure_private_files_supported() {
        aw_platform::ensure_private_dir(state_dir)
            .map_err(|_| Error::Denied("JSONL state directory is not private"))?;
    } else if !state_dir.exists() {
        std::fs::create_dir_all(state_dir)?;
    }
    aw_store_jsonl::JsonlStore::open(state_dir.join("sessions-jsonl"))
}

#[cfg(feature = "jsonl")]
impl JsonlPersistentSession {
    pub fn create(
        store: aw_store_jsonl::JsonlStore,
        actor: PrincipalId,
        binding: SessionBinding,
        title: &str,
    ) -> Result<Self> {
        let (_, _, cursor) = store.create(&actor, title, binding.clone())?;
        Ok(Self {
            store,
            actor,
            cursor,
            binding,
            allow_transfer: false,
        })
    }

    pub fn open(
        store: aw_store_jsonl::JsonlStore,
        actor: PrincipalId,
        binding: SessionBinding,
        session: &str,
        branch: &str,
        allow_transfer: bool,
    ) -> Result<Self> {
        let cursor = store.open(&actor, session, branch)?;
        let view = cursor.view()?;
        check_binding(&binding, &view.session.binding, allow_transfer)?;
        Ok(Self {
            store,
            actor,
            cursor,
            binding,
            allow_transfer,
        })
    }

    pub fn view(&self) -> Result<SessionView> {
        self.cursor.view()
    }

    pub fn apply(&self, agent: &mut Agent) -> Result<()> {
        let view = self.view()?;
        agent.load_history(view.history, Some(self.cursor.journal()))
    }

    pub fn session_id(&self) -> &str {
        self.cursor.session_id()
    }

    pub fn branch(&self) -> Result<BranchInfo> {
        self.cursor.branch()
    }

    pub fn new_session(&self, title: &str) -> Result<Self> {
        Self::create(
            self.store.clone(),
            self.actor.clone(),
            self.binding.clone(),
            title,
        )
    }

    pub fn open_session(&self, session: &str, branch: &str) -> Result<Self> {
        Self::open(
            self.store.clone(),
            self.actor.clone(),
            self.binding.clone(),
            session,
            branch,
            self.allow_transfer && session == self.session_id(),
        )
    }

    pub fn fork(&self, name: &str, at: Option<&str>) -> Result<Self> {
        if at.is_some() {
            return Err(Error::Denied(
                "JSONL forks snapshot the current Session file; --at/NODE_ID requires SQLite",
            ));
        }
        let cursor = self
            .store
            .fork(&self.actor, self.session_id(), "main", name)?;
        Ok(Self {
            store: self.store.clone(),
            actor: self.actor.clone(),
            cursor,
            binding: self.binding.clone(),
            allow_transfer: false,
        })
    }

    pub fn list(&self) -> Result<Vec<SessionInfo>> {
        self.store.list(&self.actor)
    }

    pub fn set_title(&self, title: &str) -> Result<SessionInfo> {
        self.store.set_title(&self.actor, self.session_id(), title)
    }

    pub fn branches(&self) -> Result<Vec<BranchInfo>> {
        self.store.branches(&self.actor, self.session_id())
    }
}

pub fn check_binding(
    current: &SessionBinding,
    saved: &SessionBinding,
    allow_transfer: bool,
) -> Result<()> {
    if current.working_directory != saved.working_directory || current.tool_root != saved.tool_root
    {
        return Err(Error::Denied("resume cannot move execution directories; explicitly use the saved -C/tool root or import into a new session"));
    }
    if current.system_prompt != saved.system_prompt {
        return Err(Error::Denied(
            "saved system instructions differ; start a new session to change them",
        ));
    }
    if current.destination_fingerprint != saved.destination_fingerprint && !allow_transfer {
        return Err(Error::Denied("destination/auth binding changed; review context disclosure then explicitly use --allow-context-transfer"));
    }
    Ok(())
}

#[cfg(test)]
mod phase48_tests {
    use super::*;
    use std::fs;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("aw-phase48-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }

    fn binding(root: &Path, profile: &str) -> SessionBinding {
        SessionBinding {
            working_directory: root.to_owned(),
            tool_root: root.to_owned(),
            system_prompt: "same system".into(),
            profile_hint: profile.into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[test]
    fn saved_session_selection_uses_target_binding_without_weakening_move_guard() {
        let root_a = temp_root("a");
        let root_b = temp_root("b");
        let actor = PrincipalId::new("local:phase48-session-switch").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let current = PersistentSession::create(
            store.clone(),
            actor.clone(),
            binding(&root_a, "profile-a"),
            "A",
        )
        .unwrap();
        let target =
            PersistentSession::create(store, actor, binding(&root_b, "profile-b"), "B").unwrap();

        let denied = current.open_session(target.handle.session_id(), "main");
        assert!(
            matches!(denied, Err(Error::Denied(message)) if message.contains("resume cannot move execution directories"))
        );

        let reopened = current
            .open_saved_session(target.handle.session_id(), "main")
            .unwrap();
        assert_eq!(reopened.binding.working_directory, root_b);
        assert_eq!(reopened.binding.tool_root, root_b);
        assert_eq!(reopened.binding.profile_hint, "profile-b");

        let _ = fs::remove_dir_all(root_a);
        let _ = fs::remove_dir_all(root_b);
    }
}
