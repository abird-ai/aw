//! Lightweight, single-owner JSONL Sessions for the agent-first build.
//!
//! The same backend-neutral SessionCursor/TurnJournal used by the full product
//! records intent before any provider/tool work. Nothing here constructs
//! authority from saved data: the current host rebuilds the provider and tool
//! policy, then checks exact destination/cwd/root/instruction compatibility.
use aw::{
    config::{self, Settings},
    context::ExecutionContext,
    Agent, Error, LocalToolOptions, PrincipalId, Result,
};
use aw_agent_core::CompactionPolicy;
use aw_store_api::{SessionBackend, SessionBinding, SessionCursor, SessionInfo};
use aw_store_jsonl::JsonlStore;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub enum Selection<'a> {
    New,
    Latest,
    Exact(&'a str),
}

pub struct Sessions {
    store: JsonlStore,
    actor: PrincipalId,
    current: Option<Arc<dyn SessionCursor>>,
}

pub fn default_directory() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("AW_STATE_DIR") {
        return Ok(PathBuf::from(value));
    }
    let root = if let Some(value) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(value)
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
            Error::Config("HOME/XDG_STATE_HOME missing; set --state-dir or --state-memory-only".into())
        })?)
        .join(".local/state")
    };
    Ok(root.join("abird/aw"))
}

impl Sessions {
    pub fn open(directory: Option<&Path>) -> Result<Self> {
        let root = directory
            .map(Path::to_path_buf)
            .map(Ok)
            .unwrap_or_else(default_directory)?;
        if aw_platform::secure_private_files_supported() {
            aw_platform::ensure_private_dir(&root)
                .map_err(|_| Error::Denied("JSONL state directory must be private"))?;
        } else {
            std::fs::create_dir_all(&root)?;
        }
        Ok(Self {
            store: JsonlStore::open(root.join("sessions-jsonl"))?,
            actor: PrincipalId::new(aw_platform::local_principal()?)?,
            current: None,
        })
    }

    pub fn list(&self) -> Result<Vec<SessionInfo>> {
        self.store.list(&self.actor)
    }

    pub fn current_id(&self) -> Option<&str> {
        self.current.as_ref().map(|c| c.session_id())
    }

    pub fn binding(
        settings: &Settings,
        context: &ExecutionContext,
        tools: &LocalToolOptions,
    ) -> Result<SessionBinding> {
        Ok(SessionBinding {
            working_directory: context.working_directory.clone(),
            tool_root: tools.root.canonicalize()?,
            system_prompt: settings.system_prompt.clone(),
            profile_hint: settings.profile_name.clone(),
            model_hint: settings.profile.model.clone().ok_or_else(|| {
                Error::Config("model required before creating a JSONL Session".into())
            })?,
            destination_fingerprint: config::destination_fingerprint(&settings.profile)?,
        })
    }

    fn validate_binding(saved: &SessionBinding, current: &SessionBinding) -> Result<()> {
        if saved.working_directory != current.working_directory
            || saved.tool_root != current.tool_root
            || saved.system_prompt != current.system_prompt
            || saved.profile_hint != current.profile_hint
            || saved.destination_fingerprint != current.destination_fingerprint
        {
            return Err(Error::Denied(
                "saved JSONL Session differs in directory, tools, instructions or provider custody; start a new Session explicitly",
            ));
        }
        // model_hint is historical metadata, not a model authority grant.
        // The host selects the new allowed model, and serialized history never
        // restores provider-private continuation into another model.
        Ok(())
    }

    fn ensure_no_other_unresolved_effects(
        &self,
        root: &Path,
        selected: Option<&str>,
    ) -> Result<()> {
        // JSONL does not have SQLite's root coordination table. Conservatively
        // inspect other locally owned Sessions only at the explicit activation
        // boundary, before allowing fresh work at the same tool root.
        for session in self.store.list(&self.actor)? {
            if session.binding.tool_root != root || selected == Some(session.id.as_str()) {
                continue;
            }
            let view = self.store.open(&self.actor, &session.id, "main")?.view()?;
            if view.history.needs_reconciliation {
                return Err(Error::Denied(
                    "another JSONL Session at this tool root has uncertain external effects; inspect and acknowledge that Session first",
                ));
            }
        }
        Ok(())
    }

    pub fn activate(
        &mut self,
        agent: &mut Agent,
        settings: &Settings,
        context: &ExecutionContext,
        tools: &LocalToolOptions,
        selection: Selection<'_>,
    ) -> Result<String> {
        let current = Self::binding(settings, context, tools)?;
        let cursor: Arc<dyn SessionCursor> = match selection {
            Selection::New => {
                self.ensure_no_other_unresolved_effects(&current.tool_root, None)?;
                let (_, _, cursor) =
                    self.store
                        .create(&self.actor, "New Agent", current.clone())?;
                cursor
            }
            Selection::Exact(session) => self.store.open(&self.actor, session, "main")?,
            Selection::Latest => {
                let session = self
                    .store
                    .list(&self.actor)?
                    .into_iter()
                    .find(|session| session.binding.working_directory == current.working_directory)
                    .ok_or(Error::Config(
                        "no saved JSONL Sessions for this directory; start a new Session".into(),
                    ))?;
                self.store.open(&self.actor, &session.id, "main")?
            }
        };
        let view = cursor.view()?;
        Self::validate_binding(&view.session.binding, &current)?;
        self.ensure_no_other_unresolved_effects(&current.tool_root, Some(cursor.session_id()))?;
        let id = cursor.session_id().to_owned();
        agent.load_history(view.history, Some(cursor.journal()))?;
        self.current = Some(cursor);
        Ok(id)
    }

    pub fn restore_current(
        &self,
        agent: &mut Agent,
        settings: &Settings,
        context: &ExecutionContext,
        tools: &LocalToolOptions,
    ) -> Result<()> {
        if let Some(cursor) = &self.current {
            let view = cursor.view()?;
            Self::validate_binding(
                &view.session.binding,
                &Self::binding(settings, context, tools)?,
            )?;
            agent.load_history(view.history, Some(cursor.journal()))?;
        }
        Ok(())
    }

    /// Explicit tool-free model summary with exact-current provider lease,
    /// durable paid intent and append-only CAS. The previous public Turns
    /// remain in the JSONL file; only the active projection advances.
    pub async fn compact(
        &mut self,
        agent: &mut Agent,
        settings: &Settings,
        context: &ExecutionContext,
        tools: &LocalToolOptions,
        policy: &CompactionPolicy,
    ) -> Result<usize> {
        let cursor = self.current.as_ref().ok_or(Error::Config(
            "no active JSONL Session; use --session-resume or start an explicit Session".into(),
        ))?;
        let view = cursor.view()?;
        Self::validate_binding(
            &view.session.binding, &Self::binding(settings, context, tools)?,
        )?;
        let through = aw::jsonl_compaction::compact(agent, cursor, policy).await?;
        Ok(through as usize)
    }

    pub fn set_title(&self, title: &str) -> Result<()> {
        let session = self
            .current_id()
            .ok_or_else(|| Error::Config("no active JSONL Session".into()))?;
        self.store.set_title(&self.actor, session, title)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw::config::{ProviderKind, TokenField};

    fn setup(root: &Path) -> (Settings, ExecutionContext, LocalToolOptions) {
        let context = ExecutionContext::capture(root).unwrap();
        let tools = LocalToolOptions::new(root, aw::ToolMode::Off);
        let profile = config::Profile {
            kind: ProviderKind::Ollama,
            base_url: "http://127.0.0.1:11434".into(),
            model: Some("test-model".into()),
            models: vec![],
            api_key_env: None,
            oauth_connector: None,
            oauth_host_id: None,
            allow_insecure_http: true,
            token_field: TokenField::MaxTokens,
            max_tokens: None,
            temperature: None,
            reasoning_effort: None,
            think: None,
            ca_file: None,
            include_usage: false,
        };
        let settings = Settings {
            profile_name: "test".into(),
            profile,
            system_prompt: "test system".into(),
            request_timeout: std::time::Duration::from_secs(5),
            idle_timeout: std::time::Duration::from_secs(5),
            max_output_bytes: 1024,
        };
        (settings, context, tools)
    }

    #[test]
    fn local_jsonl_activation_is_journaled_and_resume_never_runs_inference() {
        let root = std::env::temp_dir().join(format!(
            "aw-mini-session-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state = root.join("state");
        let (settings, context, tools) = setup(&root);
        let mut manager = Sessions::open(Some(&state)).unwrap();
        let mut agent = aw::configured_execution(&settings, &context, &tools, false).unwrap();
        let id = manager
            .activate(&mut agent, &settings, &context, &tools, Selection::New)
            .unwrap();
        assert!(agent.is_journaled());
        assert_eq!(manager.list().unwrap().len(), 1);
        assert_eq!(manager.current_id(), Some(id.as_str()));
        manager.set_title("Updated title").unwrap();
        assert_eq!(manager.list().unwrap()[0].title, "Updated title");
        drop(agent);
        drop(manager);

        let mut restored = Sessions::open(Some(&state)).unwrap();
        let mut agent = aw::configured_execution(&settings, &context, &tools, false).unwrap();
        assert_eq!(
            restored
                .activate(&mut agent, &settings, &context, &tools, Selection::Latest)
                .unwrap(),
            id
        );
        assert!(agent.is_journaled());
        assert!(agent.turns().is_empty());
        drop(restored);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unresolved_effects_block_new_sessions_until_exact_inspection() {
        use aw_agent_core::{
            JournalEntry, ToolCall, ToolEffect, ToolRecord, ToolStatus, TurnStart,
        };
        let root = std::env::temp_dir().join(format!(
            "aw-mini-hazard-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let (settings, context, tools) = setup(&root);
        let mut sessions = Sessions::open(Some(&root.join("state"))).unwrap();
        let mut agent = aw::configured_execution(&settings, &context, &tools, false).unwrap();
        sessions
            .activate(&mut agent, &settings, &context, &tools, Selection::New)
            .unwrap();
        let cursor = sessions.current.as_ref().unwrap().clone();
        let journal = cursor.journal();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime
            .block_on(journal.begin(TurnStart {
                turn_id: 1,
                prompt: "touch the file".into(),
                identity: agent.default_identity().unwrap(),
            }))
            .unwrap();
        // Mirror the actual Agent: first persist the original provider
        // Assistant proposal, then the irreversible Write intent.
        let mut assistant=aw_agent_core::Message::text(
            2,aw_agent_core::Role::Assistant,"");
        assistant.tool_calls=vec![ToolCall {
            id:"effect".into(),name:"write".into(),
            arguments:serde_json::json!({"path":"file.txt","content":"changed"}),
        }];
        runtime.block_on(journal.append(1,
            JournalEntry::Assistant{message:assistant})).unwrap();
        runtime
            .block_on(journal.append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "effect".into(),
                            name: "write".into(),
                            arguments: serde_json::json!({"path":"file.txt","content":"changed"}),
                        },
                        effect: ToolEffect::Write,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            ))
            .unwrap();

        let mut other = aw::configured_execution(&settings, &context, &tools, false).unwrap();
        let before = sessions.list().unwrap().len();
        assert!(sessions
            .activate(&mut other, &settings, &context, &tools, Selection::New)
            .is_err());
        assert_eq!(sessions.list().unwrap().len(), before);
        agent
            .load_history(cursor.view().unwrap().history, Some(cursor.journal()))
            .unwrap();
        assert!(agent.needs_reconciliation());
        runtime
            .block_on(agent.acknowledge_reconciliation("inspected local files"))
            .unwrap();
        assert!(!agent.needs_reconciliation());
        sessions
            .activate(&mut other, &settings, &context, &tools, Selection::New)
            .unwrap();
        assert_eq!(sessions.list().unwrap().len(), before + 1);
        drop(other);
        drop(agent);
        drop(journal);
        drop(cursor);
        drop(sessions);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lightweight_bindings_preserve_destination_fencing_but_allow_same_account_model_change() {
        let root = std::env::temp_dir().join(format!("aw-mini-bind-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (settings, context, tools) = setup(&root);
        let old = Sessions::binding(&settings, &context, &tools).unwrap();
        let mut other = settings.clone();
        other.profile.model = Some("next-model".into());
        let next = Sessions::binding(&other, &context, &tools).unwrap();
        Sessions::validate_binding(&old, &next).unwrap();
        other.profile.base_url = "http://127.0.0.2:11434".into();
        let different = Sessions::binding(&other, &context, &tools).unwrap();
        assert!(Sessions::validate_binding(&old, &different).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}