//! Host composition for root/default Agent lifecycle.
//!
//! This owns no new lifecycle state. Durable Agent state remains in
//! `SessionStore`, current-Turn cancellation remains in `actor_owner`, finite
//! work remains in `JobRuntime`, and live routing remains in `agent_jobs`.

use crate::{actor_owner, agent_jobs, Error, PrincipalId, Result};
use aw_agent_core::InvocationScope;
use aw_store::{AgentLifecycle, AgentSnapshot, QueueState, SessionStore};
use aw_store_api::SessionBackend;
use std::sync::Arc;

#[derive(Clone)]
pub struct RootAgentClose {
    store: SessionStore,
    actor: PrincipalId,
    canonical_ref: String,
    owner: actor_owner::Service,
    jobs: Option<Arc<aw_tools_adapter::job_runtime::JobRuntime>>,
    router: Arc<agent_jobs::Router>,
}

impl RootAgentClose {
    pub fn new(
        store: SessionStore,
        actor: PrincipalId,
        canonical_ref: impl Into<String>,
        owner: actor_owner::Service,
        jobs: Option<Arc<aw_tools_adapter::job_runtime::JobRuntime>>,
        router: Arc<agent_jobs::Router>,
    ) -> Result<Self> {
        let canonical_ref = canonical_ref.into();
        let (bound_actor, session, branch, _, _) = owner.bound_peer_scope();
        if bound_actor != &actor {
            return Err(Error::Denied(
                "Agent close owner principal differs from durable Agent owner",
            ));
        }
        let binding = store.agent_binding(&actor, session, branch)?;
        let expected = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            binding.domain_handle,
            binding.identity_handle,
            binding.project_handle,
            binding.agent_handle
        );
        if canonical_ref != expected {
            return Err(Error::Denied(
                "Agent close canonical ref differs from live owner binding",
            ));
        }
        Ok(Self {
            store,
            actor,
            canonical_ref,
            owner,
            jobs,
            router,
        })
    }

    pub fn canonical_ref(&self) -> &str {
        &self.canonical_ref
    }

    fn retire_pending_inputs(&self) -> Result<()> {
        let (_, session, _, _, _) = self.owner.bound_peer_scope();
        for fact in self.owner.queue_list()? {
            if fact.state != QueueState::Pending {
                continue;
            }
            let request_id = fact.request.request_id.clone();
            self.owner
                .operate(actor_owner::ActorOperation::DeleteQueue {
                    request_id: request_id.clone(),
                })?;
            if request_id.starts_with("jc_") {
                // Same conservative budget release used by stable-child close.
                // Queue custody is already gone, so failure can only leak
                // delegation capacity; it cannot resurrect runnable work.
                let _ =
                    self.store
                        .cancel_delegation_continuation(&self.actor, session, &request_id);
            }
        }
        Ok(())
    }

    pub async fn close(&self, expected_rev: Option<u64>) -> Result<AgentSnapshot> {
        let closing =
            self.store
                .agent_request_close(&self.actor, &self.canonical_ref, expected_rev)?;
        self.retire_pending_inputs()?;

        let turn_cancel = self
            .owner
            .cancel_active_turn_for_agent_close(closing.rev)
            .map(|_| ());
        let (_, session, branch, _, _) = self.owner.bound_peer_scope();
        let finite_close = match &self.jobs {
            Some(runtime) => {
                runtime
                    .close_owner(Some(&InvocationScope {
                        session_id: session.to_owned(),
                        branch_id: branch.to_owned(),
                    }))
                    .await
            }
            None => Ok(()),
        };

        // Request both independent cancellations before surfacing either
        // failure. A partial close stays durably `closing` and is retryable.
        turn_cancel?;
        finite_close?;
        self.owner.wait_turn_quiescent().await?;

        let closed = self
            .store
            .agent_finish_close(&self.actor, &self.canonical_ref)?;
        if closed.lifecycle != AgentLifecycle::Closed {
            return Err(Error::Conflict(
                "Agent terminal close returned a non-closed lifecycle",
            ));
        }
        self.router.clear_live_owner(&self.canonical_ref)?;
        Ok(closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Agent, AgentExecutionBinding, AgentOptions};
    use aw_agent_core::{
        provider::{CompletionFuture, DeltaSink, Provider},
        Message,
    };
    use aw_store::SessionBinding as StoredSessionBinding;
    use std::path::PathBuf;

    struct BlockingProvider {
        entered: Arc<tokio::sync::Notify>,
    }

    impl Provider for BlockingProvider {
        fn supports_tools(&self) -> bool {
            true
        }

        fn stream_tools<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _tools: &'a [aw_agent_core::ToolSpec],
            _sink: &'a mut DeltaSink<'_>,
        ) -> aw_agent_core::provider::ModelFuture<'a> {
            Box::pin(async move {
                self.entered.notify_one();
                std::future::pending::<()>().await;
                unreachable!()
            })
        }

        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.entered.notify_one();
                std::future::pending::<()>().await;
                unreachable!()
            })
        }
    }

    #[tokio::test]
    async fn root_close_composes_turn_finite_work_and_terminal_store_once() {
        let actor = PrincipalId::new("local").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let root = std::env::temp_dir();
        let (session, branch) = store
            .create(
                &actor,
                "root close",
                StoredSessionBinding {
                    working_directory: root.clone(),
                    tool_root: root.clone(),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();

        let entered = Arc::new(tokio::sync::Notify::new());
        let builder = Agent::builder(
            Arc::new(BlockingProvider {
                entered: entered.clone(),
            }),
            AgentOptions::new("fixture", "model"),
        );
        let mut tools = aw_tools_adapter::LocalToolOptions::new(
            PathBuf::from(&root),
            aw_tools_adapter::ToolMode::ReadOnly,
        );
        tools.selected_tools = vec!["read".into(), "batch".into()];
        let configured =
            aw_tools_adapter::configure_with_runtime(builder, actor.clone(), &tools).unwrap();
        let jobs = configured.job_runtime.clone().unwrap();
        let mut agent = configured.builder.build().unwrap();
        let (view, journal) = store.activate(&actor, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(journal)))
            .unwrap();
        let binding = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let canonical = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            binding.domain_handle,
            binding.identity_handle,
            binding.project_handle,
            binding.agent_handle
        );
        agent
            .bind_execution_owner(
                AgentExecutionBinding::from_trusted_host(
                    canonical.clone(),
                    binding.session_id,
                    binding.branch_id,
                )
                .unwrap(),
            )
            .unwrap();

        let owner =
            actor_owner::Service::attach_for_agent(store.clone(), &session.id, &branch.id, &agent)
                .unwrap();
        let router = Arc::new(agent_jobs::Router::new(store.clone(), actor.clone(), 8).unwrap());
        router
            .register_live_owner(&canonical, owner.clone(), agent.tool_binding_authority())
            .unwrap();
        let close = RootAgentClose::new(
            store.clone(),
            actor.clone(),
            canonical.clone(),
            owner.clone(),
            Some(jobs),
            router,
        )
        .unwrap();

        owner.submit_bound_text(&agent, "close me").unwrap();
        let run_owner = owner.clone();
        let task = tokio::spawn(async move {
            run_owner
                .run_once(&mut agent, |_| Ok(()))
                .await
                .unwrap()
                .unwrap()
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
            .await
            .expect("provider must enter before root close");

        let closed = tokio::time::timeout(std::time::Duration::from_secs(3), close.close(None))
            .await
            .expect("root close must reach factual terminal state")
            .unwrap();
        assert_eq!(closed.lifecycle, AgentLifecycle::Closed);
        assert_eq!(closed.admission, aw_store::AgentAdmission::Held);
        let turn = task.await.unwrap();
        assert_eq!(turn.outcome, aw_agent_core::Outcome::Cancelled);

        let replay = close.close(None).await.unwrap();
        assert_eq!(replay.lifecycle, AgentLifecycle::Closed);
        assert_eq!(replay.rev, closed.rev);
    }
}
