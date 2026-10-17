//! Host-authorized, read-only access to an existing local SQLite transcript.
//! Neither this capability nor its owned data can activate an Agent or journal
//! an effect. A returned snapshot is data, never a continuing permission grant.
use crate::{Error, Result};
use aw_agent_core::{ContextBlock, PrincipalId, Turn};
use aw_store::{SessionExecutionTarget, SessionStore};
use std::sync::Arc;

const MAX_VIEW_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub store_id: String,
    pub session_id: String,
    pub branch_id: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub target: Target,
    pub title: String,
    pub branch_name: String,
    pub head: String,
    pub revision: u64,
    pub turns: Arc<Vec<Turn>>,
    pub context_blocks: Vec<ContextBlock>,
    pub needs_inspection: bool,
    /// Precomputed off the UI thread for its aggregate retained-payload budget.
    pub retained_bytes: usize,
}

#[derive(Clone)]
pub struct Reader {
    store: SessionStore,
    actor: PrincipalId,
    store_id: String,
}

impl Reader {
    /// Only the authenticated host owner supplies an existing store and trusted actor.
    /// This never opens a State directory or creates/selects a Session.
    pub fn new(store: SessionStore, actor: PrincipalId) -> Result<Self> {
        let store_id = store.store_id()?;
        Ok(Self {
            store,
            actor,
            store_id,
        })
    }

    pub fn store_id(&self) -> &str {
        &self.store_id
    }

    fn check_store(&self, target: &Target) -> Result<()> {
        if target.store_id != self.store_id || self.store.store_id()? != self.store_id {
            return Err(Error::Denied(
                "Agent view belongs to a different State authority",
            ));
        }
        Ok(())
    }

    fn check_local(&self, target: &Target) -> Result<()> {
        if !matches!(
            self.store.session_target(&self.actor, &target.session_id)?,
            SessionExecutionTarget::Local { .. }
        ) {
            return Err(Error::Denied("remote Agent views are unavailable"));
        }
        Ok(())
    }

    /// Metadata-only current authorization for the UI's exact-current-owner
    /// projection. It returns no history and does not relax strict read fences.
    /// Tree authorization conservatively checks all derived/source releases.
    pub fn authorize(&self, target: &Target) -> Result<()> {
        self.check_store(target)?;
        self.check_local(target)?;
        let branches = self.store.branches(&self.actor, &target.session_id)?;
        if !branches.iter().any(|branch| branch.id == target.branch_id) {
            return Err(Error::Denied("Agent view branch is unavailable"));
        }
        self.check_store(target)
    }

    /// Every refresh rechecks current policy even when head/revision is equal.
    /// Non-current targets always use this path, including while the owner is busy.
    pub fn read(&self, target: &Target) -> Result<Snapshot> {
        self.check_store(target)?;
        self.check_local(target)?;
        let view =
            self.store
                .read_branch_view(&self.actor, &target.session_id, &target.branch_id)?;
        self.check_store(target)?;
        let mut turns = view.history.turns;
        for turn in &mut turns {
            turn.prompt = crate::terminal::safe_text(&turn.prompt);
            turn.answer = crate::terminal::safe_text(&turn.answer);
            turn.model = crate::terminal::safe_text(&turn.model);
            turn.profile = crate::terminal::safe_text(&turn.profile);
            turn.error = turn.error.as_deref().map(crate::terminal::safe_text);
            // Provider/account receipt metadata is not transcript presentation.
            turn.invocations.clear();
            if let Some(meta) = &mut turn.meta {
                meta.finish_reason = crate::terminal::safe_text(&meta.finish_reason);
            }
            for record in &mut turn.tool_records {
                record.execution_site = crate::terminal::safe_text(&record.execution_site);
                if let Some(output) = &mut record.output {
                    output.content = crate::terminal::safe_text(&output.content);
                }
            }
            for message in &mut turn.context_messages {
                message.content = crate::terminal::safe_text(&message.content);
                message.provider_context = None;
            }
        }
        let mut context_blocks = view.history.context_blocks;
        for block in &mut context_blocks {
            block.text = crate::terminal::safe_text(&block.text);
        }
        let title = crate::terminal::safe_text(&view.session.title);
        let branch_name = crate::terminal::safe_text(&view.branch.name);
        let retained_bytes = serde_json::to_vec(&(
            &target.store_id,
            &target.session_id,
            &target.branch_id,
            &title,
            &branch_name,
            &view.branch.head,
            &turns,
            &context_blocks,
        ))
        .map_err(|_| Error::Storage("Agent view could not be encoded"))?
        .len();
        if retained_bytes > MAX_VIEW_BYTES {
            return Err(Error::Limit(
                "Agent view exceeds the bounded transcript budget",
            ));
        }
        Ok(Snapshot {
            target: target.clone(),
            title,
            branch_name,
            head: view.branch.head,
            revision: view.branch.revision,
            turns: Arc::new(turns),
            context_blocks,
            needs_inspection: view.history.needs_reconciliation,
            retained_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::PrincipalId;
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Message, Provider,
        TurnJournal, TurnStart,
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct FixtureProvider(Arc<AtomicUsize>, Arc<AtomicBool>);
    impl Provider for FixtureProvider {
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            _: &'a [aw_agent_core::ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::ModelFuture<'a> {
            Box::pin(async move {
                let meta = self.stream(messages, model, sink).await?;
                self.1.store(true, Ordering::Relaxed);
                Ok(aw_agent_core::ModelReply {
                    meta,
                    tool_calls: vec![],
                    provider_context: Some("provider-private-continuation-sentinel".into()),
                })
            })
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::Relaxed);
                sink("answer\u{1b}]52;c;data\u{7}")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    ..Default::default()
                })
            })
        }
    }

    fn fixture() -> (
        Reader,
        Target,
        Agent,
        Arc<AtomicUsize>,
        aw_store::SessionHandle,
        Arc<AtomicBool>,
    ) {
        let store = SessionStore::in_memory().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let supplied_private = Arc::new(AtomicBool::new(false));
        let mut agent = Agent::builder(
            Arc::new(FixtureProvider(calls.clone(), supplied_private.clone())),
            AgentOptions::new("fixture", "model"),
        )
        .build()
        .unwrap();
        let actor = agent.actor().clone();
        let (session, branch) = store
            .create(
                &actor,
                "Fixture",
                aw_store::SessionBinding {
                    working_directory: "/fixture".into(),
                    tool_root: "/fixture".into(),
                    system_prompt: String::new(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let (view, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle.clone())))
            .unwrap();
        let reader = Reader::new(store, actor).unwrap();
        let target = Target {
            store_id: reader.store_id().into(),
            session_id: session.id,
            branch_id: branch.id,
        };
        (reader, target, agent, calls, handle, supplied_private)
    }

    #[tokio::test]
    async fn reader_returns_only_sanitized_bounded_history_without_a_new_call() {
        let (reader, target, mut agent, calls, _, supplied_private) = fixture();
        agent
            .prompt("prompt\u{1b}[2J", std::future::pending::<()>(), |_| Ok(()))
            .await
            .unwrap();
        assert!(supplied_private.load(Ordering::Relaxed));
        let before = reader
            .store
            .branches(&reader.actor, &target.session_id)
            .unwrap()[0]
            .clone();
        for _ in 0..2 {
            let view = reader.read(&target).unwrap();
            assert_eq!(view.target, target);
            assert_eq!(view.head, before.head);
            assert_eq!(view.revision, before.revision);
            assert_eq!(view.turns.len(), 1);
            assert!(!view.turns[0].answer.contains('\u{1b}'));
            assert!(!view.turns[0].prompt.contains('\u{1b}'));
            assert!(view.turns[0].invocations.is_empty());
            assert!(view.turns[0]
                .context_messages
                .iter()
                .all(|message| message.provider_context.is_none()));
            assert!(view.retained_bytes > 0 && view.retained_bytes <= MAX_VIEW_BYTES);
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            reader
                .store
                .branches(&reader.actor, &target.session_id)
                .unwrap()[0]
                .revision,
            before.revision
        );
    }

    #[tokio::test]
    async fn metadata_authorization_never_relaxes_strict_active_run_reads() {
        let (reader, target, agent, calls, handle, _) = fixture();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "held".into(),
                identity: agent.default_identity().unwrap(),
            })
            .await
            .unwrap();
        reader.authorize(&target).unwrap();
        assert!(matches!(reader.read(&target), Err(Error::Conflict(_))));
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        let mut foreign = target.clone();
        foreign.store_id = "different-State".into();
        assert!(reader.authorize(&foreign).is_err());
        assert!(reader.read(&foreign).is_err());
        let mut alias = target.clone();
        alias.branch_id = "main".into();
        assert!(reader.authorize(&alias).is_err());
        assert!(reader.read(&alias).is_err());
    }

    #[test]
    fn each_authorization_rechecks_revocation_with_unchanged_head() {
        let (owner, target, _, _, _, _) = fixture();
        let member = PrincipalId::new("reader-member").unwrap();
        owner
            .store
            .grant(&owner.actor, &target.session_id, &member, Some(false))
            .unwrap();
        let reader = Reader::new(owner.store.clone(), member.clone()).unwrap();
        let before = reader.read(&target).unwrap();
        reader.authorize(&target).unwrap();
        owner
            .store
            .grant(&owner.actor, &target.session_id, &member, None)
            .unwrap();
        assert!(reader.authorize(&target).is_err());
        assert!(reader.read(&target).is_err());
        let after = owner.read(&target).unwrap();
        assert_eq!((after.head, after.revision), (before.head, before.revision));
    }
}
