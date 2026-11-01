//! One live Job session for AW's canonical Graph runtime.
//!
//! This is host coordination only: Job lifecycle/output stay in `JobManager`.
//! The runtime retains only runner cancellation tokens and replaces the whole
//! live Job namespace when the authenticated Agent Session scope changes.
use crate::{
    job_host::{GraphParallelism, JobHost, JobHostConfig},
    shell, Error, Result,
};
use aw_agent_core::tool::ToolBindingAuthority;
use aw_agent_core::{CancellationToken, InvocationScope, InvocationScopeLifecycle, ToolContext};
use aw_tools::filesystem::job_manager::{Limits, Snapshot};
use futures_util::FutureExt as _;
use serde_json::Value;
use std::{
    collections::HashMap,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, Weak},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum LiveScope {
    Memory { session: String, branch: String },
    Journal(InvocationScope),
}
impl LiveScope {
    fn memory() -> Result<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes)
            .map_err(|_| Error::Config("OS randomness unavailable for Job runtime".into()))?;
        let suffix = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self::Memory {
            session: format!("memory-{suffix}"),
            branch: "main".into(),
        })
    }
    fn session(&self) -> &str {
        match self {
            Self::Memory { session, .. } => session,
            Self::Journal(scope) => &scope.session_id,
        }
    }
    fn branch(&self) -> &str {
        match self {
            Self::Memory { branch, .. } => branch,
            Self::Journal(scope) => &scope.branch_id,
        }
    }
    fn matches_context(&self, scope: Option<&InvocationScope>) -> bool {
        match (self, scope) {
            (Self::Memory { .. }, None) => true,
            (Self::Journal(bound), Some(scope)) => bound == scope,
            _ => false,
        }
    }
}

struct State {
    generation: u64,
    scope: LiveScope,
    host: Arc<JobHost>,
    runners: HashMap<String, CancellationToken>,
}

pub struct StartedJob {
    pub id: String,
    pub host: Arc<JobHost>,
    pub task: tokio::task::JoinHandle<Result<Snapshot>>,
}
pub enum StartDecision {
    New(StartedJob),
    Existing { id: String, host: Arc<JobHost> },
    Unknown,
}

pub struct JobRuntime {
    root: Arc<aw_platform::scoped_fs::Root>,
    authority: ToolBindingAuthority,
    tool_limits: crate::LocalToolLimits,
    shells: shell::ShellRegistry,
    native_tools: Arc<Vec<aw_tools::filesystem::schema::Tool>>,
    parallelism: Arc<GraphParallelism>,
    state: Mutex<State>,
}
impl JobRuntime {
    pub fn new(
        root: Arc<aw_platform::scoped_fs::Root>,
        authority: ToolBindingAuthority,
        limits: Limits,
    ) -> Result<Arc<Self>> {
        Self::new_with_native_tools(
            root,
            authority,
            crate::LocalToolLimits {
                jobs: limits,
                ..Default::default()
            },
            crate::LocalToolConcurrency::default(),
            aw_tools::filesystem::schema::Tool::GRAPH_NATIVE.to_vec(),
        )
    }

    pub(crate) fn new_with_native_tools(
        root: Arc<aw_platform::scoped_fs::Root>,
        authority: ToolBindingAuthority,
        tool_limits: crate::LocalToolLimits,
        concurrency: crate::LocalToolConcurrency,
        native_tools: Vec<aw_tools::filesystem::schema::Tool>,
    ) -> Result<Arc<Self>> {
        let scope = LiveScope::memory()?;
        let shells = shell::ShellRegistry::default();
        let native_tools = Arc::new(native_tools);
        let tool_limits = tool_limits.validate()?;
        let concurrency = concurrency.validate()?;
        if concurrency.graph_width > tool_limits.jobs.max_active_nodes {
            return Err(Error::Config(
                "Graph parallel ceiling exceeds Job active-node ceiling".into(),
            ));
        }
        let parallelism = Arc::new(GraphParallelism::new(concurrency)?);
        let host = Arc::new(JobHost::new_with_config(
            root.clone(),
            authority.clone(),
            JobHostConfig {
                session: scope.session().to_owned(),
                branch: scope.branch().to_owned(),
                tool_limits,
                shells: shells.clone(),
                native_tools: native_tools.clone(),
                parallelism: parallelism.clone(),
            },
        )?);
        Ok(Arc::new(Self {
            root,
            authority,
            tool_limits,
            shells,
            native_tools,
            parallelism,
            state: Mutex::new(State {
                generation: 1,
                scope,
                host,
                runners: HashMap::new(),
            }),
        }))
    }

    fn build_host(&self, scope: &LiveScope) -> Result<Arc<JobHost>> {
        Ok(Arc::new(JobHost::new_with_config(
            self.root.clone(),
            self.authority.clone(),
            JobHostConfig {
                session: scope.session().to_owned(),
                branch: scope.branch().to_owned(),
                tool_limits: self.tool_limits,
                shells: self.shells.clone(),
                native_tools: self.native_tools.clone(),
                parallelism: self.parallelism.clone(),
            },
        )?))
    }

    pub(crate) fn register_shell(&self, shell: Arc<shell::NativeShell>) -> Result<()> {
        self.shells.register(shell)
    }

    pub(crate) fn job_list_limit(&self) -> usize {
        self.tool_limits
            .jobs
            .max_retained_jobs
            .min(aw_tools::filesystem::job_manager::MAX_JOB_LIST_PAGE)
    }
    pub(crate) fn result_limit(&self) -> usize {
        self.tool_limits.result_bytes
    }

    pub(crate) fn node_tools(&self) -> Vec<aw_tools::filesystem::batch::NodeTool> {
        let mut tools = self
            .native_tools
            .iter()
            .copied()
            .map(aw_tools::filesystem::batch::NodeTool::Native)
            .collect::<Vec<_>>();
        tools.extend(self.shells.node_tools());
        tools
    }

    pub(crate) fn validate_graph(&self, input: &Value) -> Result<()> {
        let host = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?
            .host
            .clone();
        host.validate_graph(input)
    }

    fn current_host(&self, ctx: &ToolContext) -> Result<(u64, Arc<JobHost>)> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        if !state.scope.matches_context(ctx.scope.as_ref()) {
            return Err(Error::Denied(
                "Job runtime is bound to another live Session; finish scope replacement first",
            ));
        }
        Ok((state.generation, state.host.clone()))
    }

    fn runner_finished(&self, generation: u64, id: &str) {
        if let Ok(mut state) = self.state.lock() {
            if state.generation == generation {
                state.runners.remove(id);
            }
        }
    }

    fn spawn_runner(
        self: &Arc<Self>,
        generation: u64,
        host: Arc<JobHost>,
        ctx: &ToolContext,
        id: String,
        token: CancellationToken,
    ) -> tokio::task::JoinHandle<Result<Snapshot>> {
        let mut runner_ctx = ctx.clone();
        runner_ctx.cancellation = token.clone();
        let host_for_task = host.clone();
        let runtime: Weak<Self> = Arc::downgrade(self);
        let runner_id = id.clone();
        tokio::spawn(async move {
            let run = AssertUnwindSafe(host_for_task.run_graph(&runner_ctx, &runner_id, token))
                .catch_unwind()
                .await;
            let result = match run {
                Ok(result) => result,
                Err(_) => {
                    let _ = host_for_task.settle_runner_abort(&runner_id);
                    Err(Error::Protocol(
                        "native Job runner panicked; effect state fenced",
                    ))
                }
            };
            if let Some(runtime) = runtime.upgrade() {
                runtime.runner_finished(generation, &runner_id);
            }
            result
        })
    }

    /// Admit and start one prepared Graph under the exact outer ToolContext.
    /// `expose=true` implements immediate durable/live handoff; optimistic batch
    /// starts hidden and may expose the same Job later.
    pub fn start_graph(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
    ) -> Result<StartedJob> {
        let (generation, host) = self.current_host(ctx)?;
        let prepared = host.prepare(ctx, ops)?;
        let token = CancellationToken::new();
        let id = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation
                || !Arc::ptr_eq(&state.host, &host)
                || !state.scope.matches_context(ctx.scope.as_ref())
            {
                return Err(Error::Denied("Job Session changed during Graph preflight"));
            }
            let id = host.admit(prepared)?;
            if expose {
                host.expose(&id)?;
            }
            if state.runners.insert(id.clone(), token.clone()).is_some() {
                return Err(Error::Protocol("duplicate Job runner identity"));
            }
            id
        };

        let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token);
        Ok(StartedJob { id, host, task })
    }

    /// Replay-safe model-facing Graph constructor. The live replay claim is
    /// committed before Job admission and before any root node can start.
    pub fn start_replay(
        self: &Arc<Self>,
        ctx: &ToolContext,
        ops: &Value,
        expose: bool,
    ) -> Result<StartDecision> {
        // A journaled AW invocation already has a durable outer ToolStarted
        // replay/fencing identity. Do not create a redundant per-Job replay
        // ledger entry for it; the same outer journal prevents redispatch after
        // process replacement or response loss. Memory-only callers have no
        // such authority and use the bounded live JobManager replay ledger.
        if ctx.scope.is_some() {
            return self.start_graph(ctx, ops, expose).map(StartDecision::New);
        }
        let (generation, host) = self.current_host(ctx)?;
        let prepared = host.prepare(ctx, ops)?;
        let namespace = format!(
            "job-create:{}:{}",
            ctx.scope
                .as_ref()
                .map_or_else(|| "memory".to_owned(), |scope| scope.session_id.clone()),
            ctx.scope
                .as_ref()
                .map_or_else(|| "main".to_owned(), |scope| scope.branch_id.clone())
        );
        let decision = {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation
                || !Arc::ptr_eq(&state.host, &host)
                || !state.scope.matches_context(ctx.scope.as_ref())
            {
                return Err(Error::Denied("Job Session changed during Graph preflight"));
            }
            host.admit_replay(prepared, &namespace, &ctx.call_id)?
        };
        match decision {
            crate::job_host::AdmissionDecision::Unknown => Ok(StartDecision::Unknown),
            crate::job_host::AdmissionDecision::Existing(id) => {
                if expose {
                    host.expose(&id)?;
                }
                Ok(StartDecision::Existing { id, host })
            }
            crate::job_host::AdmissionDecision::New(id) => {
                if expose {
                    host.expose(&id)?;
                }
                let token = CancellationToken::new();
                {
                    let mut state = self
                        .state
                        .lock()
                        .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
                    if state.generation != generation || !Arc::ptr_eq(&state.host, &host) {
                        let _ = host.revoke(&id);
                        return Err(Error::Denied("Job Session changed during replay admission"));
                    }
                    if state.runners.insert(id.clone(), token.clone()).is_some() {
                        return Err(Error::Protocol("duplicate Job runner identity"));
                    }
                }
                let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token);
                Ok(StartDecision::New(StartedJob { id, host, task }))
            }
        }
    }

    pub fn cancel(&self, ctx: &ToolContext, id: &str) -> Result<(bool, Snapshot)> {
        let (_, host) = self.current_host(ctx)?;
        let before = host
            .manager()
            .get(id)
            .map_err(|_| Error::Denied("Job is not exposed or missing"))?;
        host.cancel(id)?;
        let token = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?
            .runners
            .get(id)
            .cloned();
        if let Some(token) = token {
            token.cancel();
        }
        let after = host
            .manager()
            .get(id)
            .map_err(|_| Error::Protocol("Job disappeared after cancellation"))?;
        Ok((after.sequence != before.sequence, after))
    }

    pub fn restart_graph(
        self: &Arc<Self>,
        ctx: &ToolContext,
        source_id: &str,
    ) -> Result<(crate::job_host::AdmissionDecision, Option<StartedJob>)> {
        let (generation, host) = self.current_host(ctx)?;
        let decision = host.restart(ctx, source_id)?;
        let id = match &decision {
            crate::job_host::AdmissionDecision::New(id) => id.clone(),
            _ => return Ok((decision, None)),
        };
        let token = CancellationToken::new();
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
            if state.generation != generation || !Arc::ptr_eq(&state.host, &host) {
                let _ = host.revoke(&id);
                return Err(Error::Denied(
                    "Job Session changed during restart admission",
                ));
            }
            if state.runners.insert(id.clone(), token.clone()).is_some() {
                return Err(Error::Protocol("duplicate restarted Job runner identity"));
            }
        }
        let task = self.spawn_runner(generation, host.clone(), ctx, id.clone(), token);
        Ok((
            decision,
            Some(StartedJob {
                id: id.clone(),
                host,
                task,
            }),
        ))
    }

    pub fn host_for(&self, ctx: &ToolContext) -> Result<Arc<JobHost>> {
        self.current_host(ctx).map(|(_, host)| host)
    }
}

impl InvocationScopeLifecycle for JobRuntime {
    fn validate_tool_parallelism(
        &self,
        next: aw_agent_core::ToolParallelism,
    ) -> aw_agent_core::Result<()> {
        next.validate()?;
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        if !state.runners.is_empty()
            || state
                .host
                .manager()
                .has_nonterminal_jobs()
                .map_err(|_| Error::Protocol("cannot inspect live Job session"))?
        {
            return Err(Error::Conflict(
                "tool parallel policy cannot change while Jobs are live",
            ));
        }
        Ok(())
    }

    fn apply_tool_parallelism(&self, next: aw_agent_core::ToolParallelism) {
        self.parallelism.apply(next);
    }

    fn replace_scope(
        &self,
        previous: Option<&InvocationScope>,
        next: Option<&InvocationScope>,
    ) -> aw_agent_core::Result<()> {
        if previous == next {
            return Ok(());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Protocol("Job runtime state poisoned"))?;
        for (id, token) in &state.runners {
            let _ = state.host.revoke(id);
            token.cancel();
        }
        if !state.runners.is_empty()
            || state
                .host
                .manager()
                .has_nonterminal_jobs()
                .map_err(|_| Error::Protocol("cannot inspect live Job session"))?
        {
            return Err(Error::Conflict(
                "live Jobs are cancelling for Session replacement; retry after they settle",
            ));
        }
        let scope = match next {
            Some(scope) => LiveScope::Journal(scope.clone()),
            None => LiveScope::memory()?,
        };
        let host = self.build_host(&scope)?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(Error::Limit("Job runtime scope generations exhausted"))?;
        state.scope = scope;
        state.host = host;
        state.runners.clear();
        Ok(())
    }
}

impl Drop for JobRuntime {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            for (id, token) in &state.runners {
                let _ = state.host.revoke(id);
                token.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{configure, LocalToolOptions, ToolMode};
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, DeltaSink, Message, PrincipalId, Provider,
        ToolEffect, ToolSource,
    };
    use std::fs;
    #[cfg(unix)]
    use std::path::PathBuf;

    struct UnusedProvider;
    impl Provider for UnusedProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }

    #[tokio::test]
    async fn memory_runtime_runs_hidden_job_and_scope_replacement_discards_terminal_namespace() {
        let root = std::env::temp_dir().join(format!("aw-job-runtime-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "batch".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
        };
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["read",{"path":"a.txt","numbered":false}]]}),
                false,
            )
            .unwrap();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(snapshot.finished, 1);
        assert!(started.host.manager().get(&started.id).is_err());
        runtime
            .replace_scope(
                None,
                Some(&InvocationScope {
                    session_id: "s".into(),
                    branch_id: "main".into(),
                }),
            )
            .unwrap();
        assert!(runtime.host_for(&ctx).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scope_replacement_cancels_live_runner_tokens_and_requires_retry() {
        let root =
            std::env::temp_dir().join(format!("aw-job-runtime-switch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let token = CancellationToken::new();
        runtime
            .state
            .lock()
            .unwrap()
            .runners
            .insert("fixture".into(), token.clone());
        let next = InvocationScope {
            session_id: "s".into(),
            branch_id: "main".into(),
        };
        assert!(runtime.replace_scope(None, Some(&next)).is_err());
        assert!(token.is_cancelled());
        {
            let mut state = runtime.state.lock().unwrap();
            assert!(matches!(state.scope, LiveScope::Memory { .. }));
            state.runners.remove("fixture");
        }
        runtime.replace_scope(None, Some(&next)).unwrap();
        assert!(matches!(
            runtime.state.lock().unwrap().scope,
            LiveScope::Journal(ref scope) if scope == &next
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn bash_executable() -> PathBuf {
        std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|path| path.is_file())
            })
            .expect("shell runtime tests need AW_BASH")
    }

    #[cfg(unix)]
    fn shell_runtime_fixture() -> (PathBuf, Agent, Arc<JobRuntime>, ToolContext) {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root = std::env::temp_dir().join(format!(
            "aw-shell-job-runtime-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "native\n").unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let mut tools = LocalToolOptions::new(&root, ToolMode::Trusted);
        tools.bash = Some(bash_executable());
        let agent = configure(builder, actor.clone(), &tools)
            .unwrap()
            .build()
            .unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        runtime
            .register_shell(Arc::new(
                shell::NativeShell::new(root.clone(), bash_executable()).unwrap(),
            ))
            .unwrap();
        let ctx = ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "shell-job".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Unknown,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: agent.default_identity().unwrap(),
            scope: None,
        };
        (root, agent, runtime, ctx)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mixed_native_and_shell_graph_uses_one_job_and_separate_shell_streams() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[
                    ["read",{"path":"a.txt","numbered":false}],
                    ["bash",{"command":"printf shell-out; printf shell-err >&2"},"1"]
                ]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Ok, false, false))
        );
        assert_eq!(
            host.manager()
                .output_page(&id, 2, OutputStream::Stdout, 0, 64)
                .unwrap()
                .bytes,
            b"shell-out"
        );
        assert_eq!(
            host.manager()
                .output_page(&id, 2, OutputStream::Stderr, 0, 64)
                .unwrap()
                .bytes,
            b"shell-err"
        );
        let result = host
            .manager()
            .output_page(&id, 2, OutputStream::Result, 0, 512)
            .unwrap();
        let result = String::from_utf8(result.bytes).unwrap();
        assert!(result.starts_with("ok exit=0"), "{result}");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_node_nonzero_exit_is_graph_fail_not_protocol_retry() {
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        use aw_tools::filesystem::job_manager::OutputStream;
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{"command":"printf nope; exit 9"}]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Fail, false, false))
        );
        for stream in [
            OutputStream::Result,
            OutputStream::Stdout,
            OutputStream::Stderr,
        ] {
            let page = host.manager().output_page(&id, 1, stream, 0, 512).unwrap();
            assert!(page.writer_finished, "{stream:?} writer must be finished");
        }
        assert_eq!(
            host.manager()
                .output_page(&id, 1, OutputStream::Stdout, 0, 512)
                .unwrap()
                .bytes,
            b"nope"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_node_cancel_uses_process_stop_proof_and_settles_job() {
        let (root, _agent, runtime, mut ctx) = shell_runtime_fixture();
        ctx.call_id = "shell-cancel".into();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{"command":"while :; do :; done","timeout_secs":10}]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        runtime.cancel(&ctx, &id).unwrap();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), started.task)
            .await
            .expect("cancelled shell runner must settle")
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot.stop_reason,
            Some(aw_tools::filesystem::job::StopReason::Cancelled)
        );
        assert_eq!(
            snapshot.lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_stream_truncation_is_observability_not_graph_incomplete() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{
                    "command":"printf 123456789abcdef",
                    "max_output_bytes":8
                }]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Ok, false, false))
        );
        let stdout = host
            .manager()
            .output_page(&id, 1, OutputStream::Stdout, 0, 8)
            .unwrap();
        assert_eq!(stdout.bytes, b"12345678");
        assert!(stdout.truncated);
        assert_eq!(stdout.total_bytes, 15);
        assert_eq!(stdout.retained_bytes, 8);
        let result = host
            .manager()
            .output_page(&id, 1, OutputStream::Result, 0, 512)
            .unwrap();
        let result = String::from_utf8(result.bytes).unwrap();
        assert!(result.contains("stdout_truncated=true"), "{result}");
        assert!(result.contains("stdout=15"), "{result}");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_write_hint_conflict_orders_lower_shell_before_native_read() {
        use aw_tools::filesystem::job_manager::OutputStream;
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        fs::write(root.join("x.txt"), "old").unwrap();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[
                    ["bash",{
                        "command":"sleep 0.03; printf new > x.txt",
                        "writes":["x.txt"]
                    },"*"],
                    ["read",{"path":"x.txt","numbered":false},"*"]
                ]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        let host = started.host.clone();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.summary,
            Some((aw_tools::filesystem::status::Primary::Ok, false, false))
        );
        assert_eq!(
            host.manager()
                .output_page(&id, 2, OutputStream::Result, 0, 16)
                .unwrap()
                .bytes,
            b"new"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_job_runtime_refuses_parallel_policy_change_until_quiescent() {
        let root =
            std::env::temp_dir().join(format!("aw-job-runtime-policy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let token = CancellationToken::new();
        runtime
            .state
            .lock()
            .unwrap()
            .runners
            .insert("live".into(), token);
        let next = aw_agent_core::ToolParallelism {
            max_parallel_reads: 1,
            allow_parallel_shell: false,
            max_parallel_shell: 1,
        };
        assert!(runtime.validate_tool_parallelism(next).is_err());
        runtime.state.lock().unwrap().runners.remove("live");
        assert!(runtime.validate_tool_parallelism(next).is_ok());
        runtime.apply_tool_parallelism(next);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dropping_runtime_cancels_all_live_runner_tokens() {
        let root = std::env::temp_dir().join(format!("aw-job-runtime-drop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let builder = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        );
        let tools = LocalToolOptions::new(&root, ToolMode::ReadOnly);
        let agent = configure(builder, actor, &tools).unwrap().build().unwrap();
        let runtime = JobRuntime::new(
            Arc::new(aw_platform::scoped_fs::Root::open(&root).unwrap()),
            agent.tool_binding_authority(),
            Limits::default(),
        )
        .unwrap();
        let token = CancellationToken::new();
        runtime
            .state
            .lock()
            .unwrap()
            .runners
            .insert("live".into(), token.clone());
        drop(runtime);
        assert!(token.is_cancelled());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exact_shell_grant_revocation_stops_running_process_and_marks_job_revoked() {
        let (root, mut agent, runtime, ctx) = shell_runtime_fixture();
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{
                    "command":"while :; do :; done",
                    "timeout_secs":10
                }]]}),
                true,
            )
            .unwrap();
        let id = started.id.clone();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        agent
            .revoke_registered_source_tool(
                &ctx.actor,
                "bash",
                &aw_agent_core::ToolSource::trusted_host(),
            )
            .unwrap();
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), started.task)
            .await
            .expect("revoked shell runner must terminate")
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot.stop_reason,
            Some(aw_tools::filesystem::job::StopReason::Revoked)
        );
        assert_eq!(
            snapshot.lifecycle,
            aw_tools::filesystem::job::Lifecycle::Terminal
        );
        assert!(runtime
            .host_for(&ctx)
            .unwrap()
            .manager()
            .render_job_status(&id)
            .unwrap()
            .starts_with("revoked bash exit=cancelled"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn immediate_shell_job_cancel_before_runner_poll_never_launches_process() {
        let (root, _agent, runtime, ctx) = shell_runtime_fixture();
        let marker = root.join("launched.txt");
        let started = runtime
            .start_graph(
                &ctx,
                &serde_json::json!({"ops":[["bash",{
                    "command":"printf launched > launched.txt; sleep 1",
                    "writes":["launched.txt"]
                }]]}),
                true,
            )
            .unwrap();
        runtime.cancel(&ctx, &started.id).unwrap();
        let snapshot = started.task.await.unwrap().unwrap();
        assert_eq!(
            snapshot.stop_reason,
            Some(aw_tools::filesystem::job::StopReason::Cancelled)
        );
        assert!(
            !marker.exists(),
            "cancelled-before-runner shell must never spawn"
        );
        let _ = fs::remove_dir_all(root);
    }
}
