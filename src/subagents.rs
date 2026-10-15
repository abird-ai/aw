use aw::{
    config::Settings, Agent, AgentOptions, CancellationToken, Error, LocalToolOptions, Result,
    ToolMode,
};
use aw_agent_core::{
    InvocationIdentity, JournalEntry, JournalFuture, ProviderAccounts, ProviderSelection, Tool,
    ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSpec, TurnJournal, TurnStart,
};
#[cfg(feature = "routing")]
use aw_agent_core::{InvocationContext, RoutingExecutor, RoutingInput, RoutingJournal, RoutingResult};
#[cfg(feature = "routing")]
use aw_routing::{target as routing_target, RoutingMode};
use aw_store::{DelegationLimits, SessionStore, SubagentInfo, SubagentMode};
#[cfg(feature = "routing")]
use aw_store::{RoutingSelectionSource, RoutingStrategyRevisionInfo};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_ACTIVE_BACKGROUND: usize = 4;
const MAX_TOOL_ANSWER_BYTES: usize = 96 * 1024;

#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
    lineage: Option<Lineage>,
}

#[derive(Clone, Debug)]
struct Lineage {
    root_id: String,
    allocation_id: String,
    depth: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct BudgetConfig {
    limits: DelegationLimits,
}
impl BudgetConfig {
    pub fn new(
        max_children_created: u32,
        max_active_children: u32,
        max_model_attempts: u32,
        max_depth: u8,
        deadline_seconds: u32,
    ) -> Result<Self> {
        Ok(Self {
            limits: DelegationLimits {
                max_children_created,
                max_active_children,
                max_model_attempts,
                max_depth,
                deadline_seconds,
            }
            .validate()?,
        })
    }
}
impl Default for BudgetConfig {
    fn default() -> Self {
        Self::new(32, 4, 64, 1, 3600).expect("default delegation budget is valid")
    }
}

#[cfg(feature = "routing")]
#[derive(Clone)]
pub struct ChildRoutingConfig {
    strategy: RoutingStrategyRevisionInfo,
    profiles: std::collections::BTreeMap<String, aw::config::Profile>,
    executor: RoutingExecutor,
    idle_timeout: Duration,
    request_timeout: Duration,
}
#[cfg(feature = "routing")]
impl ChildRoutingConfig {
    pub fn new(
        strategy: RoutingStrategyRevisionInfo,
        file_config: &aw::config::FileConfig,
        executor: RoutingExecutor,
        idle_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self> {
        if strategy.definition.role != "worker" {
            return Err(Error::Config(
                "subagent routing strategy must have role=worker".into(),
            ));
        }
        let profiles = crate::routing_runtime::preflight_strategy(file_config, &strategy.definition)?;
        Ok(Self {
            strategy,
            profiles,
            executor,
            idle_timeout,
            request_timeout,
        })
    }
}
#[cfg(not(feature = "routing"))]
#[derive(Clone)]
pub struct ChildRoutingConfig;

#[derive(Clone)]
pub struct ServiceConfig {
    child_tool_mode: ToolMode,
    budget: BudgetConfig,
    child_routing: Option<ChildRoutingConfig>,
    state_dir: PathBuf,
    trusted_git: Option<PathBuf>,
    child_actors: Option<crate::child_actors::Service>,
}
impl ServiceConfig {
    pub fn new(
        child_tool_mode: ToolMode,
        budget: BudgetConfig,
        state_dir: PathBuf,
        trusted_git: Option<PathBuf>,
    ) -> Self {
        Self {
            child_tool_mode,
            budget,
            child_routing: None,
            state_dir,
            trusted_git,
            child_actors: None,
        }
    }

    pub fn with_child_routing(mut self, child_routing: Option<ChildRoutingConfig>) -> Self {
        self.child_routing = child_routing;
        self
    }
    /// Explicit host-only opt-in to Actor metadata/control for existing
    /// durable child Sessions; the ordinary child path remains unchanged.
    #[allow(dead_code)] // AW-P6-7 hosted opt-in until AW-P6-9 root Session owner wiring
    pub fn with_child_actors(mut self, service: crate::child_actors::Service) -> Self {
        self.child_actors=Some(service);
        self
    }
}

struct Inner {
    store: SessionStore,
    accounts: ProviderAccounts,
    settings: Settings,
    child_tool_mode: ToolMode,
    budget: BudgetConfig,
    #[cfg(feature = "routing")]
    child_routing: Option<ChildRoutingConfig>,
    #[cfg(feature = "workspace")]
    state_dir: PathBuf,
    #[cfg(feature = "workspace")]
    git: Option<PathBuf>,
    active_background: AtomicUsize,
    child_actors: Option<crate::child_actors::Service>,
}

struct BudgetJournal {
    inner: Arc<dyn TurnJournal>,
    store: SessionStore,
    actor: aw_agent_core::PrincipalId,
    root_id: String,
    child_session_id: String,
}
impl TurnJournal for BudgetJournal {
    fn validate_context(&self) -> JournalFuture<'_, ()> {
        self.inner.validate_context()
    }
    fn validate_context_as<'a>(&'a self, identity: &'a InvocationIdentity) -> JournalFuture<'a, ()> {
        self.inner.validate_context_as(identity)
    }
    fn additional_context(&self) -> JournalFuture<'_, Vec<aw_agent_core::ContextBlock>> {
        self.inner.additional_context()
    }
    fn requires_output_gate(&self) -> JournalFuture<'_, bool> {
        self.inner.requires_output_gate()
    }
    fn invocation_scope(&self) -> Result<Option<aw_agent_core::InvocationScope>> {
        self.inner.invocation_scope()
    }
    fn begin<'a>(&'a self, start: TurnStart) -> JournalFuture<'a, ()> {
        self.inner.begin(start)
    }
    fn append<'a>(&'a self, turn_id: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            if let JournalEntry::InvocationStarted { ordinal, identity } = &entry {
                self.store.reserve_delegation_model_attempt(
                    &self.actor,
                    &self.root_id,
                    &self.child_session_id,
                    turn_id,
                    *ordinal,
                    identity,
                )?;
            }
            if let Err(error) = self.inner.append(turn_id, entry.clone()).await {
                if let JournalEntry::InvocationStarted { ordinal, .. } = &entry {
                    let detail = format!("ordinary invocation intent failed after budget reservation: {error}");
                    let bounded = detail.chars().take(2048).collect::<String>();
                    let _ = self.store.mark_delegation_model_attempt_unknown(
                        &self.actor,
                        &self.root_id,
                        &self.child_session_id,
                        turn_id,
                        *ordinal,
                        &bounded,
                    );
                }
                return Err(error);
            }
            if let JournalEntry::InvocationFinished { record } = &entry {
                self.store.finish_delegation_model_attempt(
                    &self.actor,
                    &self.root_id,
                    &self.child_session_id,
                    turn_id,
                    record,
                )?;
            }
            Ok(())
        })
    }
    fn finish<'a>(&'a self, turn: aw_agent_core::Turn) -> JournalFuture<'a, ()> {
        self.inner.finish(turn)
    }
    fn acknowledge<'a>(&'a self, note: String) -> JournalFuture<'a, ()> {
        self.inner.acknowledge(note)
    }
}

#[cfg(feature = "routing")]
struct BudgetRoutingJournal {
    inner: Arc<dyn RoutingJournal>,
    store: SessionStore,
    actor: aw_agent_core::PrincipalId,
    root_id: String,
    allocation_id: String,
    operation_id: String,
    classifier_identity: InvocationIdentity,
}
#[cfg(feature = "routing")]
impl RoutingJournal for BudgetRoutingJournal {
    fn authorize<'a>(
        &'a self,
        input: &'a RoutingInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        self.inner.authorize(input, identity)
    }

    fn begin<'a>(
        &'a self,
        input: &'a RoutingInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            if identity != &self.classifier_identity {
                return Err(Error::Denied(
                    "delegation routing classifier identity changed before dispatch",
                ));
            }
            self.store.reserve_delegation_routing_attempt(
                &self.actor,
                &self.root_id,
                &self.allocation_id,
                &self.operation_id,
                identity,
            )?;
            match self.inner.begin(input, identity).await {
                Ok(context) => Ok(context),
                Err(error) => {
                    let detail = format!(
                        "ordinary routing intent failed after delegation budget reservation: {error}"
                    );
                    let bounded = detail.chars().take(2048).collect::<String>();
                    let _ = self.store.mark_delegation_routing_attempt_unknown(
                        &self.actor,
                        &self.allocation_id,
                        &bounded,
                    );
                    Err(error)
                }
            }
        })
    }

    fn finish<'a>(&'a self, result: &'a RoutingResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            self.inner.finish(result).await?;
            self.store.finish_delegation_routing_attempt(
                &self.actor,
                &self.allocation_id,
                &self.classifier_identity,
                result,
            )?;
            Ok(())
        })
    }
}

fn unix_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnArgs {
    task: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    mode: ModeArg,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModeArg {
    #[default]
    Foreground,
    Background,
}

struct ActiveBackground {
    counter: Arc<Inner>,
}
impl Drop for ActiveBackground {
    fn drop(&mut self) {
        self.counter
            .active_background
            .fetch_sub(1, Ordering::AcqRel);
    }
}

struct Prepared {
    info: SubagentInfo,
    agent: Agent,
    selection: ProviderSelection,
    task: String,
    actor_lease: Option<crate::child_actors::ChildLease>,
}

impl Service {
    pub fn new(
        store: SessionStore,
        accounts: ProviderAccounts,
        settings: Settings,
        config: ServiceConfig,
    ) -> Result<Self> {
        let ServiceConfig {
            child_tool_mode,
            budget,
            child_routing,
            state_dir: _state_dir,
            trusted_git: _trusted_git,
            child_actors,
        } = config;
        if child_tool_mode == ToolMode::Trusted {
            return Err(Error::Denied(
                "Phase 42 child Agents do not support trusted/bash tools",
            ));
        }
        if let Some(actors) = child_actors.as_ref() {
            // Fail before private state/Git worktree provisioning when the
            // opted-in Runtime owner has not delegated exact target tool grants.
            actors.preflight_child_tool_mode(child_tool_mode)?;
        }
        #[cfg(not(feature = "routing"))]
        if child_routing.is_some() {
            return Err(Error::Denied(
                "this build does not include child routing",
            ));
        }
        #[cfg(not(feature = "workspace"))]
        if child_tool_mode != ToolMode::Off {
            return Err(Error::Denied(
                "this build needs the workspace/Git feature for tool-enabled subagents",
            ));
        }
        #[cfg(feature = "workspace")]
        let git = if child_tool_mode == ToolMode::Off {
            None
        } else {
            let git = _trusted_git.ok_or_else(|| {
                Error::Config("tool-enabled subagents require an explicit trusted Git executable".into())
            })?;
            if !git.is_absolute() || !git.is_file() {
                return Err(Error::Config(
                    "tool-enabled subagents require an absolute trusted AW_GIT executable".into(),
                ));
            }
            aw_platform::ensure_private_dir(&_state_dir)
                .map_err(|_| Error::Denied("subagent state directory is not private"))?;
            let subagents = _state_dir.join("subagents");
            aw_platform::ensure_private_dir(&subagents)
                .map_err(|_| Error::Denied("subagent state directory is not private"))?;
            let worktrees = subagents.join("worktrees");
            aw_platform::ensure_private_dir(&worktrees)
                .map_err(|_| Error::Denied("subagent worktree directory is not private"))?;
            Some(git.canonicalize()?)
        };
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                accounts,
                settings,
                child_tool_mode,
                budget,
                #[cfg(feature = "routing")]
                child_routing,
                #[cfg(feature = "workspace")]
                state_dir: _state_dir,
                #[cfg(feature = "workspace")]
                git,
                active_background: AtomicUsize::new(0),
                child_actors,
            }),
            lineage: None,
        })
    }

    fn for_child(&self, root_id: String, allocation_id: String, depth: u8) -> Self {
        Self {
            inner: self.inner.clone(),
            lineage: Some(Lineage {
                root_id,
                allocation_id,
                depth,
            }),
        }
    }

    pub fn tool(&self) -> Arc<dyn Tool> {
        Arc::new(self.clone())
    }
    #[cfg(all(feature = "pipelines", feature = "routing"))]
    pub fn delegation_limits(&self) -> DelegationLimits {
        self.inner.budget.limits
    }

    #[cfg(all(feature = "pipelines", feature = "routing"))]
    pub fn child_route_revision(&self) -> Option<&str> {
        self.child_routing_revision_id()
    }

    /// Optional Actor Step authority over existing paid decision executors.
    /// Store journal/owner budgets remain mandatory even when this is absent.
    #[cfg(feature = "pipelines")]
    pub fn actor_authority(&self) -> Option<crate::child_actors::Service> {
        self.inner.child_actors.clone()
    }
    pub fn authorize_decision(
        &self,
        ctx: &ToolContext,
        kind: aw_agent_core::DecisionKind,
        selected: &ProviderSelection,
    ) -> Result<()> {
        if let Some(actors) = &self.inner.child_actors {
            actors.authorize_decision_step(ctx, kind, selected)?;
        }
        Ok(())
    }
    fn exact_selection(&self, ctx: &ToolContext) -> Result<ProviderSelection> {
        let selected = self
            .inner
            .accounts
            .select(&ctx.actor, &ctx.identity.account_id, &ctx.identity.model)?;
        if selected.identity() != &ctx.identity {
            return Err(Error::Denied(
                "parent provider/account/model authority changed; spawn was not dispatched",
            ));
        }
        Ok(selected)
    }

    fn child_routing_revision_id(&self) -> Option<&str> {
        #[cfg(feature = "routing")]
        {
            self.inner
                .child_routing
                .as_ref()
                .map(|routing| routing.strategy.revision_id.as_str())
        }
        #[cfg(not(feature = "routing"))]
        {
            None
        }
    }

    async fn select_child_target(
        &self,
        ctx: &ToolContext,
        root: &aw_store::DelegationRootInfo,
        allocation: &aw_store::DelegationChildAllocationInfo,
        parent_selection: ProviderSelection,
        task: &str,
    ) -> Result<ProviderSelection> {
        #[cfg(feature = "routing")]
        {
            let Some(config) = self.inner.child_routing.as_ref() else {
                return Ok(parent_selection);
            };
            if root.child_routing_revision_id.as_deref()
                != Some(config.strategy.revision_id.as_str())
            {
                return Err(Error::Conflict(
                    "delegation root child routing revision differs from host configuration",
                ));
            }
            let scope = ctx.scope.as_ref().ok_or(Error::Denied(
                "routed subagents require a durable direct parent Session/branch",
            ))?;
            let scratch = Agent::builder_from_selection(
                parent_selection.clone(),
                AgentOptions::new(
                    parent_selection.identity().account_id.clone(),
                    parent_selection.identity().model.clone(),
                ),
            )
            .build()?;
            let remaining_micros = root.deadline_at.saturating_sub(unix_micros());
            if remaining_micros <= 0 {
                return Err(Error::Limit("delegation root deadline is exhausted"));
            }
            let input = crate::routing_runtime::routing_input(
                &config.strategy.revision_id,
                &config.strategy.definition,
                "worker",
                "subagents",
                task,
                config
                    .request_timeout
                    .min(Duration::from_micros(remaining_micros as u64)),
            )?;
            let input_sha = input.digest()?;
            let candidate_sha = input.candidate_ids_digest()?;
            let strategy = &config.strategy.definition;

            let (selection, operation) = match strategy.mode {
                RoutingMode::Fixed => {
                    let target_id = strategy
                        .fixed_target
                        .as_deref()
                        .ok_or(Error::Storage("fixed child route lost its target"))?;
                    let target = routing_target(strategy, target_id)
                        .map_err(|error| Error::Config(error.to_string()))?;
                    let selection = crate::routing_runtime::register_target(
                        &scratch,
                        &self.inner.store,
                        &config.profiles,
                        target,
                        config.idle_timeout,
                    )?;
                    selection.revalidate()?;
                    let operation = self.inner.store.routing_admit_resolved(
                        &ctx.actor,
                        &config.strategy.revision_id,
                        &scope.session_id,
                        &scope.branch_id,
                        "worker",
                        "subagents",
                        &input_sha,
                        &candidate_sha,
                        RoutingSelectionSource::Fixed,
                        target_id,
                    )?;
                    (selection, operation)
                }
                RoutingMode::RouteOnce => {
                    let affinity = self.inner.store.routing_affinity(
                        &ctx.actor,
                        &scope.session_id,
                        &scope.branch_id,
                        "worker",
                        "subagents",
                    )?;
                    if let Some(affinity) = affinity
                        .as_ref()
                        .filter(|value| value.strategy_revision_id == config.strategy.revision_id)
                    {
                        let target = routing_target(strategy, &affinity.target_id)
                            .map_err(|error| Error::Config(error.to_string()))?;
                        let selection = crate::routing_runtime::register_target(
                            &scratch,
                            &self.inner.store,
                            &config.profiles,
                            target,
                            config.idle_timeout,
                        )?;
                        selection.revalidate()?;
                        let operation = self.inner.store.routing_admit_resolved(
                            &ctx.actor,
                            &config.strategy.revision_id,
                            &scope.session_id,
                            &scope.branch_id,
                            "worker",
                            "subagents",
                            &input_sha,
                            &candidate_sha,
                            RoutingSelectionSource::Affinity,
                            &affinity.target_id,
                        )?;
                        (selection, operation)
                    } else {
                        self.classify_child_target(
                            ctx,
                            root,
                            allocation,
                            &scratch,
                            config,
                            input,
                            affinity.map(|value| value.revision),
                        )
                        .await?
                    }
                }
                RoutingMode::RouteEachPrompt => {
                    self.classify_child_target(
                        ctx,
                        root,
                        allocation,
                        &scratch,
                        config,
                        input,
                        None,
                    )
                    .await?
                }
            };
            self.inner.store.routing_consume_for_delegation(
                &ctx.actor,
                &operation.id,
                &allocation.id,
                selection.identity(),
            )?;
            Ok(selection)
        }
        #[cfg(not(feature = "routing"))]
        {
            let _ = (ctx, root, allocation, task);
            Ok(parent_selection)
        }
    }

    #[cfg(feature = "routing")]
    #[allow(clippy::too_many_arguments)]
    async fn classify_child_target(
        &self,
        ctx: &ToolContext,
        root: &aw_store::DelegationRootInfo,
        allocation: &aw_store::DelegationChildAllocationInfo,
        scratch: &Agent,
        config: &ChildRoutingConfig,
        input: RoutingInput,
        affinity_expected_revision: Option<u64>,
    ) -> Result<(ProviderSelection, aw_store::RoutingOperationInfo)> {
        let strategy = &config.strategy.definition;
        let classifier = strategy
            .classifier
            .as_ref()
            .ok_or(Error::Storage("classified child route lost its classifier"))?;
        let classifier_selection = crate::routing_runtime::register_target(
            scratch,
            &self.inner.store,
            &config.profiles,
            classifier,
            config.idle_timeout,
        )?;
        self.authorize_decision(
            ctx,
            aw_agent_core::DecisionKind::Routing,
            &classifier_selection,
        )?;
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "classified child routing requires direct parent Session/branch",
        ))?;
        let operation = self.inner.store.routing_admit_classifier(
            &ctx.actor,
            &config.strategy.revision_id,
            &scope.session_id,
            &scope.branch_id,
            "worker",
            "subagents",
            &input.digest()?,
            &input.candidate_ids_digest()?,
        )?;
        let inner_journal = match self
            .inner
            .store
            .routing_classifier_journal(ctx.actor.clone(), &operation.id)
        {
            Ok(journal) => journal,
            Err(error) => {
                self.inner.store.routing_settle_unstarted(
                    &ctx.actor,
                    &operation.id,
                    matches!(error, Error::Cancelled),
                    &error.to_string(),
                )?;
                return Err(error);
            }
        };
        let journal = BudgetRoutingJournal {
            inner: Arc::new(inner_journal),
            store: self.inner.store.clone(),
            actor: ctx.actor.clone(),
            root_id: root.id.clone(),
            allocation_id: allocation.id.clone(),
            operation_id: operation.id.clone(),
            classifier_identity: classifier_selection.identity().clone(),
        };
        let result = match config
            .executor
            .execute(
                &classifier_selection,
                &input,
                &journal,
                ctx.cancellation.cancelled(),
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.inner.store.routing_settle_unstarted(
                    &ctx.actor,
                    &operation.id,
                    matches!(error, Error::Cancelled),
                    &error.to_string(),
                )?;
                return Err(error);
            }
        };
        if result.outcome != aw::conversation::Outcome::Completed {
            return Err(Error::Protocol(
                "child routing classifier did not produce one valid completed candidate; inspect `aw route status` before retrying",
            ));
        }
        let target_id = result
            .proposal
            .as_ref()
            .ok_or(Error::Protocol("completed child routing decision has no candidate"))?
            .candidate_id
            .clone();
        if strategy.mode == RoutingMode::RouteOnce {
            self.inner.store.routing_affinity_establish(
                &ctx.actor,
                &operation.id,
                affinity_expected_revision,
            )?;
        }
        let target = routing_target(strategy, &target_id)
            .map_err(|error| Error::Config(error.to_string()))?;
        let selection = crate::routing_runtime::register_target(
            scratch,
            &self.inner.store,
            &config.profiles,
            target,
            config.idle_timeout,
        )?;
        selection.revalidate()?;
        let operation = self
            .inner
            .store
            .routing_operation(&ctx.actor, &operation.id)?;
        Ok((selection, operation))
    }

    fn default_title(task: &str) -> String {
        let mut title = String::from("Subagent: ");
        for ch in task.trim().chars().take(72) {
            title.push(ch);
        }
        if title == "Subagent: " {
            "Subagent".into()
        } else {
            title
        }
    }

    fn validate_args(arguments: &Value) -> Result<SpawnArgs> {
        let args: SpawnArgs = serde_json::from_value(arguments.clone()).map_err(|_| {
            Error::Denied("subagent arguments must contain task, optional title, optional mode")
        })?;
        if args.task.trim().is_empty()
            || args.task.len() > aw::config::MAX_PROMPT_BYTES
            || args.task.chars().any(|ch| ch == '\0')
        {
            return Err(Error::Config(
                "subagent task needs 1..131072 bounded UTF-8 bytes".into(),
            ));
        }
        if let Some(title) = &args.title {
            if title.trim().is_empty()
                || title.len() > 160
                || title.chars().any(char::is_control)
            {
                return Err(Error::Config(
                    "subagent title needs 1..160 printable bytes".into(),
                ));
            }
        }
        Ok(args)
    }

    fn reserve_background(inner: &Arc<Inner>) -> Result<ActiveBackground> {
        loop {
            let current = inner.active_background.load(Ordering::Acquire);
            if current >= MAX_ACTIVE_BACKGROUND {
                return Err(Error::Limit(
                    "at most four background subagents may run at once",
                ));
            }
            if inner
                .active_background
                .compare_exchange(
                    current,
                    current + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return Ok(ActiveBackground {
                    counter: inner.clone(),
                });
            }
        }
    }

    #[cfg(feature = "workspace")]
    fn provision_execution_binding(
        &self,
        ctx: &ToolContext,
        delegation_root_id: &str,
        delegation_allocation_id: &str,
    ) -> Result<Option<String>> {
        let inner = &self.inner;
        if inner.child_tool_mode == ToolMode::Off {
            return Ok(None);
        }
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "tool-enabled subagents require a durable parent Session/branch",
        ))?;
        let parent = inner.store.session(&ctx.actor, &scope.session_id)?;
        let git = inner.git.as_ref().ok_or(Error::Denied(
            "tool-enabled subagents require a trusted Git executable",
        ))?;
        let repo = aw_git::Repository::discover(git, &parent.binding.tool_root)
            .map_err(|error| Error::Config(format!("cannot prepare isolated child worktree: {error}")))?;
        let repo_root = repo.root().canonicalize()?;
        let parent_cwd = parent.binding.working_directory.canonicalize()?;
        let parent_tool_root = parent.binding.tool_root.canonicalize()?;
        if parent_cwd.strip_prefix(&repo_root).is_err()
            || parent_tool_root.strip_prefix(&repo_root).is_err()
        {
            return Err(Error::Denied(
                "parent cwd/tool root must stay inside one Git worktree for Phase 42",
            ));
        }
        let base_revision = repo
            .clean_head_for_managed_worktree()
            .map_err(|error| Error::Config(format!("cannot snapshot parent worktree: {error}")))?;
        let managed_parent = inner.state_dir.join("subagents").join("worktrees");
        let managed_parent = managed_parent.canonicalize()?;
        if managed_parent.starts_with(&repo_root) {
            return Err(Error::Denied(
                "aw managed-worktree directory cannot live inside the parent repository",
            ));
        }
        if let Some(lineage) = &self.lineage {
            let parent_execution = inner
                .store
                .subagent_execution_binding_for_child_session(&ctx.actor, &scope.session_id)?
                .ok_or(Error::Denied(
                    "recursive tool-enabled delegation requires the direct parent managed binding",
                ))?;
            if parent_execution.state != "bound"
                || parent_execution.delegation_root_id.as_deref() != Some(&lineage.root_id)
                || parent_execution.delegation_allocation_id.as_deref()
                    != Some(&lineage.allocation_id)
                || parent_execution.execution_root.canonicalize()? != repo_root
            {
                return Err(Error::Denied(
                    "recursive tool-enabled delegation parent binding does not match lineage",
                ));
            }
        } else if repo_root.starts_with(&managed_parent) {
            return Err(Error::Denied(
                "top-level parent repository cannot already be an aw managed subagent worktree",
            ));
        }
        let binding_id = aw_store::new_subagent_execution_binding_id()?;
        let execution_root = managed_parent.join(&binding_id);
        inner.store.plan_subagent_execution_binding(
            &ctx.actor,
            &binding_id,
            &scope.session_id,
            &scope.branch_id,
            ctx.turn_id,
            delegation_root_id,
            delegation_allocation_id,
            &inner.child_tool_mode.to_string(),
            &repo_root,
            &execution_root,
            &base_revision,
        )?;
        match repo.create_managed_worktree(&execution_root, &base_revision) {
            Ok(worktree) => {
                let actual = worktree.path.canonicalize().map_err(Error::Io)?;
                if actual != execution_root || worktree.head.as_deref() != Some(&base_revision) {
                    let _ = inner.store.mark_subagent_execution_unknown(
                        &ctx.actor,
                        &binding_id,
                        "managed worktree verification differed from the planned path/revision",
                    );
                    return Err(Error::Storage(
                        "managed worktree outcome requires inspection; no automatic retry",
                    ));
                }
                inner
                    .store
                    .mark_subagent_execution_ready(&ctx.actor, &binding_id)?;
                Ok(Some(binding_id))
            }
            Err(error) => {
                let detail = format!("managed worktree creation outcome unknown: {error}");
                let _ = inner
                    .store
                    .mark_subagent_execution_unknown(&ctx.actor, &binding_id, &detail);
                Err(Error::Storage(
                    "managed worktree creation outcome requires inspection; no automatic retry",
                ))
            }
        }
    }

    #[cfg(not(feature = "workspace"))]
    fn provision_execution_binding(
        &self,
        _ctx: &ToolContext,
        _delegation_root_id: &str,
        _delegation_allocation_id: &str,
    ) -> Result<Option<String>> {
        if self.inner.child_tool_mode == ToolMode::Off {
            Ok(None)
        } else {
            Err(Error::Denied(
                "this build does not include managed Git worktrees",
            ))
        }
    }

    async fn prepare(
        &self,
        ctx: &ToolContext,
        args: SpawnArgs,
        mode: SubagentMode,
    ) -> Result<Prepared> {
        let inner = &self.inner;
        if ctx.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let scope = ctx.scope.as_ref().ok_or(Error::Denied(
            "subagents require a durable parent Session/branch",
        ))?;

        let parent_selection = {
            let selected = inner
                .accounts
                .select(&ctx.actor, &ctx.identity.account_id, &ctx.identity.model)?;
            if selected.identity() != &ctx.identity {
                return Err(Error::Denied(
                    "parent provider/account/model authority changed; spawn was not dispatched",
                ));
            }
            selected
        };

        let title = args
            .title
            .clone()
            .unwrap_or_else(|| Self::default_title(&args.task));
        let task_sha256 = format!("{:x}", Sha256::digest(args.task.as_bytes()));
        let (root, allocation) = if let Some(lineage) = &self.lineage {
            if lineage.depth >= inner.budget.limits.max_depth {
                return Err(Error::Limit("delegation recursive depth limit reached"));
            }
            inner.store.reserve_delegation_descendant(
                &ctx.actor,
                &lineage.root_id,
                &lineage.allocation_id,
                &scope.session_id,
                &scope.branch_id,
                ctx.turn_id,
                inner.budget.limits,
                self.child_routing_revision_id(),
                &ctx.identity,
            )?
        } else {
            inner.store.reserve_delegation_child(
                &ctx.actor,
                &scope.session_id,
                &scope.branch_id,
                ctx.turn_id,
                inner.budget.limits,
                self.child_routing_revision_id(),
                1,
            )?
        };
        let selected = match self
            .select_child_target(
                ctx,
                &root,
                &allocation,
                parent_selection,
                &args.task,
            )
            .await
        {
            Ok(selection) => selection,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner.store.fail_delegation_child_allocation(
                    &ctx.actor,
                    &allocation.id,
                    &bounded,
                );
                return Err(error);
            }
        };
        let execution_binding_id = match self.provision_execution_binding(
            ctx,
            &root.id,
            &allocation.id,
        ) {
            Ok(value) => value,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner.store.fail_delegation_child_allocation(
                    &ctx.actor,
                    &allocation.id,
                    &bounded,
                );
                return Err(error);
            }
        };
        let tool_mode = inner.child_tool_mode.to_string();
        let creation = inner.store.create_subagent_session(
            &ctx.actor,
            &scope.session_id,
            &scope.branch_id,
            ctx.turn_id,
            &title,
            &task_sha256,
            selected.identity(),
            &root.id,
            &allocation.id,
            allocation.depth,
            &tool_mode,
            execution_binding_id.as_deref(),
            mode,
        );
        let (info, child, child_branch) = match creation {
            Ok(value) => value,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner.store.fail_delegation_child_allocation(
                    &ctx.actor,
                    &allocation.id,
                    &bounded,
                );
                return Err(error);
            }
        };

        let mut options = AgentOptions::new(
            selected.identity().account_id.clone(),
            selected.identity().model.clone(),
        );
        let child_scope = match inner.child_tool_mode {
            ToolMode::Off => "You have no local file or shell tools.",
            ToolMode::ReadOnly => {
                "You may read bounded files only inside your isolated managed Git worktree."
            }
            ToolMode::Workspace => {
                "You may read/write/edit bounded files only inside your isolated managed Git worktree."
            }
            ToolMode::Trusted => unreachable!("constructor rejects trusted child tools"),
        };
        let recursive_scope = if allocation.depth < root.limits.max_depth {
            format!(
                "You may delegate bounded sub-tasks through the subagent tool. Descendants share this root's limits and you cannot choose their provider/model/root/tool authority. Current depth {}/{}.",
                allocation.depth, root.limits.max_depth
            )
        } else {
            format!(
                "You are at the delegation depth limit {}/{} and cannot spawn subagents.",
                allocation.depth, root.limits.max_depth
            )
        };
        options.system_prompt = format!(
            "{}

You are a bounded aw subagent in an isolated child Session. Complete only the delegated task.              Return a concise result for the parent Agent. Do not assume access to the parent transcript.              {child_scope} {recursive_scope} You have no shell/Git/network management authority.",
            inner.settings.system_prompt
        );
        let remaining_micros = root.deadline_at.saturating_sub(unix_micros());
        if remaining_micros <= 0 {
            let detail = "delegation root deadline expired before child start";
            let _ = inner.store.fail_admitted_subagent(&ctx.actor, &info.id, detail);
            return Err(Error::Limit("delegation root deadline is exhausted"));
        }
        options.request_timeout = inner
            .settings
            .request_timeout
            .min(Duration::from_micros(remaining_micros as u64));
        options.max_output_bytes = inner.settings.max_output_bytes;

        if let Err(error) = inner.store.mark_subagent_running(&ctx.actor, &info.id) {
            let detail = error.to_string();
            let bounded = detail.chars().take(2048).collect::<String>();
            let _ = inner
                .store
                .fail_admitted_subagent(&ctx.actor, &info.id, &bounded);
            return Err(error);
        }
        let setup = (|| -> Result<Agent> {
            let mut builder = Agent::builder_from_selection(selected.clone(), options);
            if inner.child_tool_mode != ToolMode::Off {
                let mut child_tools =
                    LocalToolOptions::new(child.binding.tool_root.clone(), inner.child_tool_mode);
                // Preserve the Phase-42 exact child tool set. Search on the
                // parent must not silently widen descendants' grants.
                child_tools.selected_tools = match inner.child_tool_mode {
                    ToolMode::ReadOnly => vec!["read".into()],
                    ToolMode::Workspace => vec!["read".into(), "write".into(), "edit".into()],
                    ToolMode::Off | ToolMode::Trusted => unreachable!("child mode validated"),
                };
                child_tools.execution_site = format!("subagent:{}", info.id);
                builder = aw_tools::configure(builder, ctx.actor.clone(), &child_tools)?;
            }
            let mut agent = builder.build()?;
            if allocation.depth < root.limits.max_depth {
                let descendant_service =
                    self.for_child(root.id.clone(), allocation.id.clone(), allocation.depth);
                agent.register_host_tool(descendant_service.tool())?;
            }
            let (view, handle) = inner
                .store
                .activate(&ctx.actor, &child.id, &child_branch.id)?;
            let journal: Arc<dyn TurnJournal> = Arc::new(BudgetJournal {
                inner: Arc::new(handle),
                store: inner.store.clone(),
                actor: ctx.actor.clone(),
                root_id: root.id.clone(),
                child_session_id: child.id.clone(),
            });
            agent.load_history(view.history, Some(journal))?;
            Ok(agent)
        })();
        let agent = match setup {
            Ok(agent) => agent,
            Err(error) => {
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let _ = inner
                    .store
                    .settle_subagent(&ctx.actor, &info.id, "failed", Some(&bounded));
                return Err(error);
            }
        };

        let actor_lease=if let Some(service)=&inner.child_actors {
            let turn=agent.history_state().next_turn_id;
            match service.attach(&info,&agent,&selected,&args.task,turn) {
                Ok(lease)=>Some(lease),
                Err(error)=>{
                    // The normal SessionStore remains the owner of lifecycle
                    // facts. No unbound Actor may silently execute a child.
                    let note=error.to_string();
                    let bounded=note.chars().take(2048).collect::<String>();
                    let _=inner.store.settle_subagent(&ctx.actor,&info.id,
                        "failed",Some(&bounded));
                    return Err(error);
                }
            }
        } else {None};
        Ok(Prepared {
            info,
            agent,
            selection: selected,
            task: args.task,
            actor_lease,
        })
    }

    fn bounded_answer(answer: &str) -> (String, bool) {
        if answer.len() <= MAX_TOOL_ANSWER_BYTES {
            return (answer.to_owned(), false);
        }
        let mut end = MAX_TOOL_ANSWER_BYTES;
        while !answer.is_char_boundary(end) {
            end -= 1;
        }
        (answer[..end].to_owned(), true)
    }

    async fn run_prepared(
        inner: Arc<Inner>,
        mut prepared: Prepared,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput> {
        let actor = prepared.info.identity.actor.clone();
        let result = if let Some(lease)=prepared.actor_lease.as_ref() {
            lease.run_once(&mut prepared.agent,&prepared.selection,cancellation).await
        } else {
            prepared.agent.prompt_as(&prepared.selection,prepared.task,
                cancellation.cancelled(), |_|Ok(())).await
        };

        match result {
            Ok(turn) if turn.outcome == aw::conversation::Outcome::Completed => {
                inner.store.settle_subagent(&actor,&prepared.info.id,"completed",None)?;
                if let Some(lease)=&prepared.actor_lease {
                    lease.observe_settled(turn.needs_reconciliation)?;
                }
                let (answer, truncated) = Self::bounded_answer(&turn.answer);
                let body = json!({
                    "subagent_id": prepared.info.id,
                    "session_id": prepared.info.child_session_id,
                    "state": "completed",
                    "answer": answer,
                    "answer_truncated": truncated,
                });
                Ok(ToolOutput::text(body.to_string()))
            }
            Ok(turn) => {
                let state = if turn.outcome == aw::conversation::Outcome::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                };
                let detail = turn
                    .error
                    .as_deref()
                    .unwrap_or("subagent did not complete successfully");
                inner.store.settle_subagent(&actor,&prepared.info.id,state,Some(detail))?;
                if let Some(lease)=&prepared.actor_lease {
                    lease.observe_settled(turn.needs_reconciliation)?;
                }
                Ok(ToolOutput::error(
                    json!({
                        "subagent_id": prepared.info.id,
                        "session_id": prepared.info.child_session_id,
                        "state": state,
                        "error": detail,
                    })
                    .to_string(),
                ))
            }
            Err(error) => {
                let state = if matches!(error, Error::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                };
                let detail = error.to_string();
                let bounded = detail.chars().take(2048).collect::<String>();
                let settled=inner.store.settle_subagent(
                    &actor,&prepared.info.id,state,Some(&bounded));
                if settled.is_ok() {
                    if let Some(lease)=&prepared.actor_lease {
                        // Aborted provider/effect may be unknown, never
                        // misreported as an independently settled Actor.
                        let _=lease.observe_settled(true);
                    }
                }
                Err(error)
            }
        }
    }
}

impl Tool for Service {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "subagent".into(),
            description: "Delegate one bounded task to an isolated durable child Agent Session. The trusted host may preconfigure an immutable bounded child-routing strategy, but you cannot choose provider, model, route, root, actor, budget, depth, or additional authority. Foreground waits for the result; background returns the child Session immediately.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "title": {"type": "string"},
                    "mode": {
                        "type": "string",
                        "enum": ["foreground", "background"],
                        "default": "foreground"
                    }
                },
                "required": ["task"],
                "additionalProperties": false
            }),
            effect: ToolEffect::Process,
        }
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        Self::validate_args(arguments).map(|_| ())
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args = Self::validate_args(arguments)?;
            // Revalidate the exact selected identity before creating any child state.
            self.exact_selection(&ctx)?;
            match args.mode {
                ModeArg::Foreground => {
                    let prepared = self.prepare(&ctx, args, SubagentMode::Foreground).await?;
                    Self::run_prepared(self.inner.clone(), prepared, ctx.cancellation).await
                }
                ModeArg::Background => {
                    let active = Self::reserve_background(&self.inner)?;
                    let prepared = self.prepare(&ctx, args, SubagentMode::Background).await?;
                    let response = json!({
                        "subagent_id": prepared.info.id,
                        "session_id": prepared.info.child_session_id,
                        "state": "running",
                    })
                    .to_string();
                    let inner = self.inner.clone();
                    let spawn = thread::Builder::new()
                        .name(format!(
                            "aw-subagent-{}",
                            prepared.info.id.chars().rev().take(8).collect::<String>()
                        ))
                        .spawn(move || {
                            let _active = active;
                            let runtime = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .max_blocking_threads(4)
                                .build();
                            match runtime {
                                Ok(runtime) => {
                                    let _ = runtime.block_on(Self::run_prepared(
                                        inner,
                                        prepared,
                                        CancellationToken::new(),
                                    ));
                                }
                                Err(error) => {
                                    let actor = prepared.info.identity.actor.clone();
                                    let detail = format!("cannot start subagent runtime: {error}");
                                    let _ = inner.store.settle_subagent(
                                        &actor,
                                        &prepared.info.id,
                                        "failed",
                                        Some(&detail),
                                    );
                                }
                            }
                        });
                    if let Err(error) = spawn {
                        return Err(Error::Io(error));
                    }
                    Ok(ToolOutput::text(response))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw::config;
    use aw_agent_core::{
        AccountDescriptor, CompletionFuture, CompletionMeta, DeltaSink, InvocationScope, JournalEntry,
        PrincipalId, Provider, ToolCall, ToolContext, ToolEffect, ToolRecord, ToolStatus,
        TurnJournal, TurnStart, Usage,
    };
    use aw_agent_core::{Message, ModelFuture, ModelReply, Role};
    #[cfg(feature = "workspace")]
    use std::process::Command;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    struct EchoProvider {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for EchoProvider {
        fn stream<'a>(
            &'a self,
            messages: &'a [aw_agent_core::Message],
            model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                assert!(messages.iter().any(|message| message.content == "research task"));
                assert!(!messages.iter().any(|message| message.content == "parent secret text"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("child answer")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(3),
                        output_tokens: Some(2),
                    },
                })
            })
        }
    }

    /// Never completes on its own; only an authenticated Actor Turn cancel
    /// can finish this real background Agent provider attempt.
    struct BlockingProvider {
        calls: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
    }
    impl Provider for BlockingProvider {
        fn stream<'a>(
            &'a self, messages:&'a [aw_agent_core::Message], _:&'a str,
            _:&'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert!(messages.iter().any(|m|m.content=="background task"));
                assert!(!messages.iter().any(|m|m.content=="parent secret text"));
                self.calls.fetch_add(1,Ordering::SeqCst);
                self.entered.notify_one();
                std::future::pending::<Result<CompletionMeta>>().await
            })
        }
    }

    struct RecursiveProvider {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for RecursiveProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                assert!(messages.iter().any(|message| message.content == "level2 task"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("level2 answer")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage {
                        input_tokens: Some(2),
                        output_tokens: Some(1),
                    },
                })
            })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                if names.is_empty() {
                    assert!(messages.iter().any(|message| message.content == "level2 task"));
                    self.calls.fetch_add(1, Ordering::SeqCst);
                    sink("level2 answer")?;
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(2),
                                output_tokens: Some(1),
                            },
                        },
                        ..Default::default()
                    });
                }
                assert_eq!(names, vec!["subagent"]);
                assert!(messages.iter().any(|message| message.content == "level1 task"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("recursive-leaf"));
                    let body: serde_json::Value = serde_json::from_str(&result.content).unwrap();
                    assert_eq!(body["state"], "completed");
                    assert_eq!(body["answer"], "level2 answer");
                    sink("level1 answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "recursive-leaf".into(),
                            name: "subagent".into(),
                            arguments: json!({
                                "task": "level2 task",
                                "title": "Leaf child",
                                "mode": "foreground"
                            }),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct RecursiveWorkspaceProvider {
        calls: Arc<AtomicUsize>,
    }
    #[cfg(feature = "workspace")]
    impl Provider for RecursiveWorkspaceProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("recursive workspace fixture uses tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                self.calls.fetch_add(1, Ordering::SeqCst);
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                let parent_task = messages.iter().any(|m| m.content == "workspace-parent task");
                let leaf_task = messages.iter().any(|m| m.content == "workspace-leaf task");
                if parent_task {
                    assert_eq!(names, vec!["edit", "read", "subagent", "write"]);
                    if messages.last().is_some_and(|message| message.role == Role::Tool) {
                        let result = messages.last().unwrap();
                        assert_eq!(result.tool_call_id.as_deref(), Some("workspace-recursive-leaf"));
                        let body: serde_json::Value = serde_json::from_str(&result.content).unwrap();
                        assert_eq!(body["state"], "completed");
                        assert_eq!(body["answer"], "workspace-leaf answer");
                        sink("workspace-parent answer")?;
                        return Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                usage: Usage {
                                    input_tokens: Some(5),
                                    output_tokens: Some(2),
                                },
                            },
                            ..Default::default()
                        });
                    }
                    return Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "workspace-recursive-leaf".into(),
                            name: "subagent".into(),
                            arguments: json!({
                                "task":"workspace-leaf task",
                                "title":"Workspace leaf",
                                "mode":"foreground"
                            }),
                        }],
                        provider_context: None,
                    });
                }
                assert!(leaf_task);
                assert_eq!(names, vec!["edit", "read", "write"]);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("workspace-leaf-write"));
                    assert!(!result.content.starts_with("ERROR:"));
                    sink("workspace-leaf answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            usage: Usage {
                                input_tokens: Some(3),
                                output_tokens: Some(1),
                            },
                        },
                        tool_calls: vec![ToolCall {
                            id: "workspace-leaf-write".into(),
                            name: "write".into(),
                            arguments: json!({
                                "path":"grandchild-only.txt",
                                "content":"grandchild isolated bytes\\n"
                            }),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct DirtyRecursiveWorkspaceProvider {
        calls: Arc<AtomicUsize>,
    }
    #[cfg(feature = "workspace")]
    impl Provider for DirtyRecursiveWorkspaceProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("dirty recursive fixture uses tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                assert!(messages.iter().any(|m| m.content == "dirty-parent task"));
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                assert_eq!(names, vec!["edit", "read", "subagent", "write"]);
                let last_tool = messages
                    .last()
                    .filter(|message| message.role == Role::Tool)
                    .and_then(|message| message.tool_call_id.as_deref());
                match last_tool {
                    None => Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "dirty-parent-write".into(),
                            name: "write".into(),
                            arguments: json!({
                                "path":"dirty-before-delegate.txt",
                                "content":"dirty parent bytes\\n"
                            }),
                        }],
                        provider_context: None,
                    }),
                    Some("dirty-parent-write") => Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "dirty-parent-subagent".into(),
                            name: "subagent".into(),
                            arguments: json!({
                                "task":"must never reach descendant provider",
                                "mode":"foreground"
                            }),
                        }],
                        provider_context: None,
                    }),
                    Some("dirty-parent-subagent") => {
                        let result = messages.last().unwrap();
                        assert!(result.content.contains("clean parent checkout"));
                        sink("dirty descendant correctly blocked")?;
                        Ok(ModelReply {
                            meta: CompletionMeta {
                                finish_reason: "stop".into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                    }
                    other => panic!("unexpected tool result {other:?}"),
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct WorkspaceToolProvider {
        calls: Arc<AtomicUsize>,
    }
    #[cfg(feature = "workspace")]
    impl Provider for WorkspaceToolProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("workspace fixture requires tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            model: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                assert_eq!(model, "fixture-model");
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                assert_eq!(names, vec!["edit", "read", "write"]);
                assert!(!names.contains(&"subagent"));
                assert!(!names.contains(&"bash"));
                self.calls.fetch_add(1, Ordering::SeqCst);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    let result = messages.last().unwrap();
                    assert_eq!(result.tool_call_id.as_deref(), Some("child-write"));
                    assert!(!result.content.starts_with("ERROR:"));
                    sink("child workspace answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            usage: Usage {
                                input_tokens: Some(4),
                                output_tokens: Some(2),
                            },
                        },
                        ..Default::default()
                    })
                } else {
                    assert!(messages
                        .iter()
                        .any(|message| message.content == "workspace task"));
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "child-write".into(),
                            name: "write".into(),
                            arguments: json!({
                                "path": "child-created.txt",
                                "content": "created only in child worktree\\n"
                            }),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(feature = "workspace")]
    struct ReadOnlyToolProvider;
    #[cfg(feature = "workspace")]
    impl Provider for ReadOnlyToolProvider {
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("read-only fixture requires tool adapter")) })
        }
        fn stream_tools<'a>(
            &'a self,
            messages: &'a [Message],
            _: &'a str,
            tools: &'a [ToolSpec],
            sink: &'a mut DeltaSink<'_>,
        ) -> ModelFuture<'a> {
            Box::pin(async move {
                let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
                assert_eq!(names, vec!["read"]);
                if messages.last().is_some_and(|message| message.role == Role::Tool) {
                    assert!(messages.last().unwrap().content.contains("parent bytes"));
                    sink("read-only child answer")?;
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "stop".into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                } else {
                    Ok(ModelReply {
                        meta: CompletionMeta {
                            finish_reason: "tool_calls".into(),
                            ..Default::default()
                        },
                        tool_calls: vec![ToolCall {
                            id: "child-read".into(),
                            name: "read".into(),
                            arguments: json!({"path":"base.txt","start_line":1,"max_lines":20}),
                        }],
                        provider_context: None,
                    })
                }
            })
        }
    }

    #[cfg(all(unix, feature = "workspace"))]
    struct GitFixture {
        root: PathBuf,
        repo: PathBuf,
        state: PathBuf,
        git: PathBuf,
    }
    #[cfg(all(unix, feature = "workspace"))]
    impl GitFixture {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            static SERIAL: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "aw-subagent-worktree-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            let repo = root.join("repo");
            std::fs::create_dir(&repo).unwrap();
            let state = root.join("state");
            let git = std::env::var_os("AW_GIT")
                .map(PathBuf::from)
                .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
            let run = |args: &[&str]| {
                let output = Command::new(&git)
                    .current_dir(&repo)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .args(args)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "git {args:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            };
            run(&["init", "--initial-branch=main"]);
            run(&["config", "user.name", "Fixture"]);
            run(&["config", "user.email", "fixture@example.invalid"]);
            std::fs::write(repo.join("base.txt"), b"parent bytes\\n").unwrap();
            run(&["add", "base.txt"]);
            run(&["commit", "-m", "fixture", "--no-gpg-sign"]);
            Self {
                root,
                repo: repo.canonicalize().unwrap(),
                state,
                git: git.canonicalize().unwrap(),
            }
        }
    }
    #[cfg(all(unix, feature = "workspace"))]
    impl Drop for GitFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn binding() -> aw_store::SessionBinding {
        aw_store::SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    /// Opt-in host Actor test bridge. The original SQLite parent Session is
    /// still authoritative; the root Runtime adds no provider or tool grant.
    fn actor_owner(
        store: &SessionStore,
        owner: &PrincipalId,
        root_session: &str,
        max_depth: u8,
    ) -> crate::child_actors::Service {
        actor_owner_for_mode(store, owner, root_session, max_depth, ToolMode::Off)
    }
    fn actor_owner_for_mode(
        store: &SessionStore,
        owner: &PrincipalId,
        root_session: &str,
        max_depth: u8,
        mode: ToolMode,
    ) -> crate::child_actors::Service {
        use aw_agent_core::runtime::{HostRuntime, SessionBinding};
        use aw_agent_kernel::{
            actor::{ActorState, RuntimeKey},
            capability::{Effect, SourceKind},
            envelope::{Access, Budget, EnvelopeSpec, ExactGrant, Network},
        };
        use std::{num::NonZeroU64, sync::Mutex};
        let mut grants = vec![ExactGrant {
            name: "subagent".into(),
            kind: SourceKind::Host,
            source: "trusted-host".into(),
            source_epoch: 1,
            effect: Effect::Process,
        }];
        let extra: &[(&str, Effect)] = match mode {
            ToolMode::Off => &[],
            ToolMode::ReadOnly => &[("read", Effect::Read)],
            ToolMode::Workspace => &[
                ("read", Effect::Read),
                ("write", Effect::Write),
                ("edit", Effect::Write),
            ],
            ToolMode::Trusted => panic!("fixture never issues trusted Bash"),
        };
        for (name, effect) in extra {
            grants.push(ExactGrant {
                name: (*name).into(),
                kind: SourceKind::Host,
                source: "trusted-host".into(),
                source_epoch: 1,
                effect: *effect,
            });
        }
        let nz = |value| NonZeroU64::new(value).unwrap();
        let mut runtime = HostRuntime::new(
            owner.clone(),
            "local",
            RuntimeKey {
                id: nz(81),
                generation: nz(1),
            },
            16,
            16,
        )
        .unwrap();
        let root = runtime
            .bind_agent(
                owner,
                SessionBinding::from_trusted_host(root_session, None).unwrap(),
                None,
                EnvelopeSpec {
                    actor: owner.as_str().into(),
                    site: "local".into(),
                    revision: 1,
                    access: Access::ReadWrite,
                    network: Network::Deny,
                    process: true,
                    tty: false,
                    microphone: false,
                    speaker: false,
                    budget: Budget {
                        max_model_rounds: 8,
                        max_tool_calls: 128,
                        max_children: 4,
                        max_depth,
                        max_output_bytes: 1024 * 1024,
                    },
                    grants,
                },
                "root Agent",
            )
            .unwrap();
        runtime
            .record_state(owner, root, None, ActorState::Running)
            .unwrap();
        crate::child_actors::Service::new(
            store.clone(),
            Arc::new(Mutex::new(runtime)),
            owner.clone(),
            root,
            None,
            root_session,
        )
        .unwrap()
    }

    fn settings() -> Settings {
        let mut profile = config::builtins().remove("openai").unwrap();
        profile.model = Some("fixture-model".into());
        Settings {
            profile_name: "fixture-account".into(),
            profile,
            system_prompt: "parent instructions".into(),
            request_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(5),
            max_output_bytes: aw::config::MAX_OUTPUT_BYTES,
        }
    }

    #[tokio::test]
    async fn typed_routing_planner_and_guardian_step_gate_use_same_parent_actor() {
        let owner = PrincipalId::new("local:decision-owner").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    owner.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(EchoProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let selected = accounts
            .select(&owner, "fixture-account", "fixture-model")
            .unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (root, branch) = store.create(&owner, "Root", binding()).unwrap();
        let bridge = actor_owner(&store, &owner, &root.id, 2);
        let context = ToolContext {
            actor: owner.clone(),
            turn_id: 1,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            identity: selected.identity().clone(),
            scope: Some(InvocationScope {
                session_id: root.id.clone(),
                branch_id: branch.id.clone(),
            }),
        };
        // Host Step authorization never starts a paid decision, and requires
        // a real existing SQLite active parent Turn.
        for kind in [
            aw_agent_core::DecisionKind::Planner,
            aw_agent_core::DecisionKind::Guardian,
            aw_agent_core::DecisionKind::Routing,
        ] {
            assert!(bridge
                .authorize_decision_step(&context, kind, &selected)
                .is_err());
        }
        let (_, journal) = store.activate(&owner, &root.id, &branch.id).unwrap();
        journal
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent paid attempt".into(),
                identity: selected.identity().clone(),
            })
            .await
            .unwrap();
        for kind in [
            aw_agent_core::DecisionKind::Planner,
            aw_agent_core::DecisionKind::Guardian,
            aw_agent_core::DecisionKind::Routing,
        ] {
            bridge
                .authorize_decision_step(&context, kind, &selected)
                .unwrap();
        }
        let mut forged = context.clone();
        forged.execution_site = "subagent:forged".into();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Planner, &selected)
            .is_err());
        forged = context.clone();
        forged.scope.as_mut().unwrap().branch_id = "b_nonexistent".into();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Guardian, &selected)
            .is_err());
        forged = context.clone();
        forged.actor = PrincipalId::new("local:other").unwrap();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Routing, &selected)
            .is_err());
        forged = context.clone();
        forged.cancellation.cancel();
        assert!(bridge
            .authorize_decision_step(&forged, aw_agent_core::DecisionKind::Planner, &selected)
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn explicit_actor_bridge_rejects_cross_site_worktree_before_side_effects() {
        let actor = PrincipalId::new("local:no-site-bridge").unwrap();
        let store = SessionStore::in_memory().unwrap();
        let (root, _) = store.create(&actor, "Parent", binding()).unwrap();
        let bridge = actor_owner(&store, &actor, &root.id, 2);
        let state =
            std::env::temp_dir().join(format!("aw-p6-cross-site-refusal-{}", std::process::id()));
        assert!(!state.exists());
        let accounts = ProviderAccounts::new("local").unwrap();
        let request = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::default(),
                state.clone(),
                Some(PathBuf::from("/not/approved/git")),
            )
            .with_child_actors(bridge),
        );
        assert!(matches!(request, Err(Error::Denied(_))));
        assert!(
            !state.exists(),
            "invalid cross-site Actor binding must fail before Git side effects"
        );
        let partial = actor_owner_for_mode(&store, &actor, &root.id, 2, ToolMode::ReadOnly);
        let partial_request = Service::new(
            store,
            ProviderAccounts::new("local").unwrap(),
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::default(),
                state.clone(),
                Some(PathBuf::from("/not/approved/git")),
            )
            .with_child_actors(partial),
        );
        assert!(matches!(partial_request, Err(Error::Denied(_))));
        assert!(
            !state.exists(),
            "read-only ceiling cannot create writable child worktree"
        );
    }

    #[tokio::test]
    async fn foreground_tool_uses_exact_parent_lease_and_isolated_child_session() {
        let actor = PrincipalId::new("local:test").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(EchoProvider {
            calls: calls.clone(),
        });
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                provider,
            )
            .unwrap();
        let selection = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap();
        let identity = selection.identity().clone();

        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store
            .create(&actor, "Parent", binding())
            .unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-fixture".into(), name: "subagent".into(),
            arguments: json!({"task":"research task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-fixture".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"research task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();

        let actor_service=actor_owner(&store,&actor,&parent.id,2);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                PathBuf::from("/tmp"),
                None,
            ).with_child_actors(actor_service.clone()),
        ).unwrap();
        assert!(service
            .validate(&json!({"task":"x","model":"other"}))
            .is_err());

        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity: identity.clone(),
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({
                    "task": "research task",
                    "title": "Research child",
                    "mode": "foreground"
                }),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let result: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(result["state"], "completed");
        assert_eq!(result["answer"], "child answer");

        let relations = store.subagents_for_parent(&actor, &parent.id).unwrap();
        assert_eq!(relations.len(), 1);
        let relation = &relations[0];
        assert_eq!(relation.identity, identity);
        assert_eq!(relation.state, "completed");
        let live=actor_service.list().unwrap();
        assert_eq!(live.len(),1);
        assert_eq!(live[0].0.id,relation.id);
        assert!(live[0].1.parent.is_some());
        assert_eq!(live[0].1.kind,aw_agent_kernel::actor::ActorKind::Agent);
        assert_eq!(live[0].1.state,aw_agent_kernel::actor::ActorState::Settled);
        assert_eq!(live[0].1.depth,1);
        let (stored,descriptor)=actor_service.status(&relation.id).unwrap();
        assert_eq!(stored.state,"completed");
        assert_eq!(descriptor.key,live[0].1.key);
        let fact=actor_service.wait(&relation.id,CancellationToken::new(),
            Duration::from_millis(200)).await.unwrap();
        assert_eq!(fact.state,"completed");
        assert!(actor_service.signal(&relation.id,
            aw_agent_kernel::message::ControlKind::Cancel(
                aw_agent_kernel::message::Scope::Turn),1).is_err());
        assert!(actor_service.release_settled(&relation.id).is_err(),
            "cannot discard a final parent notice before it is consumed");
        let notice=actor_service.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(notice.from,descriptor.key);
        assert_eq!(notice.kind,aw_agent_kernel::message::Kind::Notify);
        assert!(!notice.payload.contains("child answer"));
        assert!(!notice.payload.contains("parent secret text"));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&notice.payload)
            .unwrap()["state"],"completed");
        assert!(actor_service.poll_root_notification(1).unwrap().is_none());
        actor_service.release_settled(&relation.id).unwrap();
        assert!(actor_service.list().unwrap().is_empty());
        assert!(actor_service.status(&relation.id).is_err());
        assert_eq!(store.subagent(&actor,&relation.id).unwrap().state,"completed");


        let (view, _) = store
            .activate(&actor, &relation.child_session_id, "main")
            .unwrap();
        assert_eq!(view.session.title, "Research child");
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].prompt, "research task");
        assert_eq!(view.history.turns[0].answer, "child answer");
        assert!(!view
            .history
            .turns
            .iter()
            .any(|turn| turn.prompt == "parent secret text"));

        // The parent turn is still a separate active turn; the service did not
        // mutate or complete it while running the child.
        assert!(store.branches(&actor, &parent.id).unwrap()[0]
            .active_run
            .is_some());
    }

    #[tokio::test]
    async fn attached_background_child_cancel_is_fenced_and_never_replayed() {
        use aw_agent_kernel::message::{ControlKind, Scope};
        let actor=PrincipalId::new("local:child-cancel").unwrap();
        let calls=Arc::new(AtomicUsize::new(0));
        let entered=Arc::new(tokio::sync::Notify::new());
        let accounts=ProviderAccounts::new("local").unwrap();
        accounts.register(AccountDescriptor::personal(
            "fixture-account",actor.clone(),"local",["fixture-model".into()]
        ),Arc::new(BlockingProvider{
            calls:calls.clone(),entered:entered.clone()
        })).unwrap();
        let identity=accounts.select(&actor,"fixture-account","fixture-model")
            .unwrap().identity().clone();
        let store=SessionStore::in_memory().unwrap();
        let (parent,branch)=store.create(&actor,"Parent",binding()).unwrap();
        let (_,journal)=store.activate(&actor,&parent.id,&branch.id).unwrap();
        journal.begin(TurnStart{
            turn_id:1,prompt:"parent secret text".into(),identity:identity.clone()
        }).await.unwrap();
        let mut proposal=Message::text(2,Role::Assistant,"");
        proposal.tool_calls=vec![ToolCall{
            id:"bg-child".into(),name:"subagent".into(),
            arguments:json!({"task":"background task","mode":"background"}),
        }];
        journal.append(1,JournalEntry::Assistant{message:proposal}).await.unwrap();
        journal.append(1,JournalEntry::ToolStarted{record:ToolRecord{
            call:ToolCall{id:"bg-child".into(),name:"subagent".into(),
                arguments:json!({"task":"background task","mode":"background"})},
            effect:ToolEffect::Process,execution_site:"local".into(),
            status:ToolStatus::Running,output:None
        },
                source: None,}).await.unwrap();
        let actors=actor_owner(&store,&actor,&parent.id,2);
        let service=Service::new(store.clone(),accounts,settings(),
            ServiceConfig::new(ToolMode::Off,BudgetConfig::default(),
                PathBuf::from("/tmp"),None).with_child_actors(actors.clone())).unwrap();
        let response=service.execute(ToolContext{
            actor:actor.clone(),turn_id:1,execution_site:"local".into(),
            cancellation:CancellationToken::new(),identity,
            scope:Some(InvocationScope{
                session_id:parent.id.clone(),branch_id:branch.id.clone()
            }),
        },&json!({"task":"background task","mode":"background"})).await.unwrap();
        assert!(!response.is_error);
        let response:serde_json::Value=serde_json::from_str(&response.content).unwrap();
        let id=response["subagent_id"].as_str().unwrap();
        assert_eq!(response["state"],"running");
        tokio::time::timeout(Duration::from_secs(3),entered.notified())
            .await.expect("original child provider never started");
        assert_eq!(calls.load(Ordering::SeqCst),1);
        // Broad or stale signals are never executed as child cancellation.
        assert!(matches!(actors.signal(id,ControlKind::Freeze(Scope::Turn),1),
            Ok(aw_agent_core::runtime::Delivery::Applied(_))));
        assert!(matches!(actors.signal(id,ControlKind::Resume(Scope::Turn),1),
            Ok(aw_agent_core::runtime::Delivery::Applied(_))));
        assert!(actors.signal(id,ControlKind::Cancel(Scope::Subtree),1).is_err());
        assert!(actors.signal(id,ControlKind::Cancel(Scope::Turn),2).is_err());
        assert!(matches!(actors.signal(id,ControlKind::Cancel(Scope::Turn),1),
            Ok(aw_agent_core::runtime::Delivery::Applied(_))));
        let settled=actors.wait(id,CancellationToken::new(),
            Duration::from_secs(3)).await.unwrap();
        assert_eq!(settled.state,"cancelled");
        assert_eq!(calls.load(Ordering::SeqCst),1);
        assert_eq!(actors.status(id).unwrap().1.state,
            aw_agent_kernel::actor::ActorState::Cancelled);
        assert!(actors.signal(id,ControlKind::Cancel(Scope::Turn),1).is_err());
        let notice=actors.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&notice.payload)
            .unwrap()["state"],"cancelled");
        assert!(!notice.payload.contains("background task"));
        assert!(actors.poll_root_notification(1).unwrap().is_none());
    }

    #[tokio::test]
    async fn recursive_model_only_child_delegates_to_depth_two_under_same_root() {
        let actor = PrincipalId::new("local:recursive-off").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(RecursiveProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store.create(&actor, "Parent", binding()).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-recursive-root".into(), name: "subagent".into(),
            arguments: json!({"task":"level1 task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-recursive-root".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"level1 task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let actor_service=actor_owner(&store,&actor,&parent.id,2);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::new(4, 2, 8, 2, 3600).unwrap(),
                PathBuf::from("/tmp"),
                None,
            ).with_child_actors(actor_service.clone()),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({"task":"level1 task","title":"First child","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        let body: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(body["answer"], "level1 answer");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let live=actor_service.list().unwrap();
        assert_eq!(live.len(),2);
        let depth_one=live.iter().find(|(_,a)|a.depth==1).unwrap();
        let depth_two=live.iter().find(|(_,a)|a.depth==2).unwrap();
        assert_eq!(depth_two.1.parent,Some(depth_one.1.key));
        assert_eq!(depth_one.1.state,aw_agent_kernel::actor::ActorState::Settled);
        assert_eq!(depth_two.1.state,aw_agent_kernel::actor::ActorState::Settled);

        let first = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(first.depth, Some(1));
        assert_eq!(first.state, "completed");
        let second = store
            .subagents_for_parent(&actor, &first.child_session_id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(second.depth, Some(2));
        assert_eq!(second.state, "completed");
        assert!(actor_service.release_settled(&second.id).is_err(),
            "nested parent's unconsumed final notice must block release");
        let nested_notice=actor_service.poll_child_notification(&first.id,1)
            .unwrap().unwrap();
        assert_eq!(nested_notice.from,depth_two.1.key);
        assert_eq!(nested_notice.to,depth_one.1.key);
        assert!(nested_notice.payload.contains("completed"));
        assert!(!nested_notice.payload.contains("level2 answer"));
        actor_service.release_settled(&second.id).unwrap();
        assert!(actor_service.release_settled(&first.id).is_err());
        let root_notice=actor_service.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(root_notice.from,depth_one.1.key);
        assert!(!root_notice.payload.contains("level1 answer"));
        actor_service.release_settled(&first.id).unwrap();
        assert!(actor_service.list().unwrap().is_empty());
        assert_eq!(store.subagent(&actor,&first.id).unwrap().state,"completed");
        assert_eq!(store.subagent(&actor,&second.id).unwrap().state,"completed");

        assert_eq!(second.delegation_root_id, first.delegation_root_id);
        let root = store
            .delegation_root(&actor, first.delegation_root_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(root.children_created, 2);
        assert_eq!(root.active_children, 0);
        assert_eq!(root.model_attempts_started, 3);
        assert_eq!(root.model_attempts_finished, 3);
        assert_eq!(root.limits.max_depth, 2);
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn recursive_workspace_child_gets_clean_sibling_worktree_and_same_root() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:recursive-workspace").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(RecursiveWorkspaceProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-recursive-workspace".into(),
            name: "subagent".into(),
            arguments: json!({"task":"workspace-parent task"}),
        }];
        handle
            .append(1, JournalEntry::Assistant { message: assistant })
            .await
            .unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-recursive-workspace".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"workspace-parent task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let actor_service =
            actor_owner_for_mode(&store, &actor, &parent.id, 2, ToolMode::Workspace);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::new(4, 3, 8, 2, 3600).unwrap(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            )
            .with_child_actors(actor_service.clone()),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({"task":"workspace-parent task","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert!(!fixture.repo.join("grandchild-only.txt").exists());

        let first = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let second = store
            .subagents_for_parent(&actor, &first.child_session_id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(first.depth, Some(1));
        assert_eq!(second.depth, Some(2));
        let first_actor = actor_service.status(&first.id).unwrap().1;
        let second_actor = actor_service.status(&second.id).unwrap().1;
        assert_eq!(first_actor.depth, 1);
        assert_eq!(second_actor.depth, 2);
        assert_eq!(second_actor.parent, Some(first_actor.key));
        assert_eq!(
            first_actor.state,
            aw_agent_kernel::actor::ActorState::Settled
        );
        assert_eq!(
            second_actor.state,
            aw_agent_kernel::actor::ActorState::Settled
        );
        let first_env = actor_service.envelope(&first.id).unwrap();
        let second_env = actor_service.envelope(&second.id).unwrap();
        assert_eq!(first_env.site(), format!("subagent:{}", first.id));
        assert_eq!(second_env.site(), format!("subagent:{}", second.id));
        assert_ne!(first_env.site(), second_env.site());
        assert_eq!(
            first_env.spec().network,
            aw_agent_kernel::envelope::Network::Deny
        );
        assert_eq!(
            second_env.spec().network,
            aw_agent_kernel::envelope::Network::Deny
        );
        assert!(actor_service.release_settled(&second.id).is_err());
        let nested = actor_service
            .poll_child_notification(&first.id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(nested.from, second_actor.key);
        assert_eq!(nested.to, first_actor.key);
        assert!(!nested.payload.contains("grandchild isolated bytes"));
        actor_service.release_settled(&second.id).unwrap();
        assert!(actor_service.release_settled(&first.id).is_err());
        let root_notice = actor_service.poll_root_notification(1).unwrap().unwrap();
        assert_eq!(root_notice.from, first_actor.key);
        actor_service.release_settled(&first.id).unwrap();
        assert!(actor_service.list().unwrap().is_empty());

        let first_exec = store
            .subagent_execution_binding_for_subagent(&actor, &first.id)
            .unwrap()
            .unwrap();
        let second_exec = store
            .subagent_execution_binding_for_subagent(&actor, &second.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            first_exec.execution_root.parent(),
            second_exec.execution_root.parent()
        );
        assert!(!second_exec
            .execution_root
            .starts_with(&first_exec.execution_root));
        assert!(!first_exec
            .execution_root
            .join("grandchild-only.txt")
            .exists());
        assert_eq!(
            std::fs::read(second_exec.execution_root.join("grandchild-only.txt")).unwrap(),
            b"grandchild isolated bytes\\n"
        );
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        let first_repo =
            aw_git::Repository::discover(&fixture.git, &first_exec.execution_root).unwrap();
        assert!(first_repo.status().unwrap().changes.is_empty());
        assert!(repo.status().unwrap().changes.is_empty());
        let root = store
            .delegation_root(&actor, first.delegation_root_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(root.children_created, 2);
        assert_eq!(root.active_children, 0);
        assert_eq!(root.model_attempts_started, 4);
        assert_eq!(root.limits.max_depth, 2);

        let cleanup_second = store
            .begin_subagent_execution_cleanup(&actor, &second.id)
            .unwrap();
        repo.remove_managed_worktree(&cleanup_second.execution_root, true)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup_second.id)
            .unwrap();
        let cleanup_first = store
            .begin_subagent_execution_cleanup(&actor, &first.id)
            .unwrap();
        repo.remove_managed_worktree(&cleanup_first.execution_root, false)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup_first.id)
            .unwrap();
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn dirty_recursive_parent_consumes_allocation_but_never_dispatches_descendant() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:dirty-recursive").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(DirtyRecursiveWorkspaceProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "root".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "root-dirty-recursive".into(), name: "subagent".into(),
            arguments: json!({"task":"dirty-parent task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "root-dirty-recursive".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"dirty-parent task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::new(4, 2, 8, 2, 3600).unwrap(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            ),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({"task":"dirty-parent task","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("managed child worktree requires a clean parent checkout"));
        assert_eq!(calls.load(Ordering::SeqCst), 2, "descendant provider must never run");
        let first = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(store.subagents_for_parent(&actor, &first.child_session_id).unwrap().len(), 0);
        let root = store
            .delegation_root(&actor, first.delegation_root_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(root.children_created, 2, "failed descendant allocation stays consumed");
        assert_eq!(root.active_children, 0);
        let first_exec = store
            .subagent_execution_binding_for_subagent(&actor, &first.id)
            .unwrap()
            .unwrap();
        assert!(first_exec.execution_root.join("dirty-before-delegate.txt").exists());
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &first.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, true)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn workspace_child_edits_only_managed_worktree_and_retains_it_for_review() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:workspace-child").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(WorkspaceToolProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-workspace".into(),
            name: "subagent".into(),
            arguments: json!({"task":"workspace task"}),
        }];
        handle
            .append(1, JournalEntry::Assistant { message: assistant })
            .await
            .unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-workspace".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"workspace task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();

        let actor_service =
            actor_owner_for_mode(&store, &actor, &parent.id, 1, ToolMode::Workspace);
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::default(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            )
            .with_child_actors(actor_service.clone()),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({
                    "task":"workspace task",
                    "title":"Editing child",
                    "mode":"foreground"
                }),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!fixture.repo.join("child-created.txt").exists());
        assert_eq!(
            std::fs::read(fixture.repo.join("base.txt")).unwrap(),
            b"parent bytes\\n"
        );

        let relation = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(relation.state, "completed");
        assert_eq!(relation.tool_mode, "workspace");
        let (actor_info, descriptor) = actor_service.status(&relation.id).unwrap();
        assert_eq!(actor_info.child_session_id, relation.child_session_id);
        assert_eq!(descriptor.depth, 1);
        assert_eq!(
            descriptor.state,
            aw_agent_kernel::actor::ActorState::Settled
        );
        let bound = actor_service.envelope(&relation.id).unwrap();
        assert_eq!(bound.site(), format!("subagent:{}", relation.id));
        assert_eq!(
            bound.spec().network,
            aw_agent_kernel::envelope::Network::Deny
        );
        assert_eq!(bound.spec().grants.len(), 3);
        assert!(bound
            .spec()
            .grants
            .iter()
            .all(|g| g.kind == aw_agent_kernel::capability::SourceKind::Host));
        let execution = store
            .subagent_execution_binding_for_subagent(&actor, &relation.id)
            .unwrap()
            .unwrap();
        assert_eq!(execution.state, "bound");
        assert!(execution.execution_root.starts_with(&fixture.state));
        assert_eq!(
            std::fs::read(execution.execution_root.join("child-created.txt")).unwrap(),
            b"created only in child worktree\\n"
        );
        let (view, _) = store
            .activate(&actor, &relation.child_session_id, "main")
            .unwrap();
        assert_eq!(view.session.binding.tool_root, execution.execution_root);
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].tool_records.len(), 1);
        assert_eq!(view.history.turns[0].tool_records[0].call.name, "write");
        assert!(view.history.turns[0]
            .tool_records
            .iter()
            .all(|record| record.call.name != "subagent" && record.call.name != "bash"));

        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        assert!(matches!(
            repo.remove_managed_worktree(&cleanup.execution_root, false),
            Err(aw_git::Error::Stale)
        ));
        let restored = store
            .cancel_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
        assert_eq!(restored.state, "bound");
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, true)
            .unwrap();
        let removed = store
            .finish_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
        assert_eq!(removed.state, "removed");
        assert_eq!(
            store
                .session(&actor, &relation.child_session_id)
                .unwrap()
                .id,
            relation.child_session_id
        );
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn read_only_child_advertises_only_read_and_cleans_without_discard() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:read-only-child").unwrap();
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(ReadOnlyToolProvider),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "delegate".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-read-only".into(), name: "subagent".into(),
            arguments: json!({"task":"read task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-read-only".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"read task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::ReadOnly,
                BudgetConfig::default(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            ),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({"task":"read task","mode":"foreground"}),
            )
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(result["answer"], "read-only child answer");
        let relation = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(relation.tool_mode, "read-only");
        let execution = store
            .subagent_execution_binding_for_subagent(&actor, &relation.id)
            .unwrap()
            .unwrap();
        assert!(execution.execution_root.join("base.txt").exists());
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, false)
            .unwrap();
        assert_eq!(
            store
                .finish_subagent_execution_cleanup(&actor, &cleanup.id)
                .unwrap()
                .state,
            "removed"
        );
        assert!(store.session(&actor, &relation.child_session_id).is_ok());
    }

    #[cfg(all(unix, feature = "workspace"))]
    #[tokio::test]
    async fn delegation_model_attempt_limit_blocks_second_provider_round() {
        let fixture = GitFixture::new();
        let actor = PrincipalId::new("local:budget-limit-child").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(WorkspaceToolProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let parent_binding = aw_store::SessionBinding {
            working_directory: fixture.repo.clone(),
            tool_root: fixture.repo.clone(),
            system_prompt: "parent instructions".into(),
            profile_hint: "fixture-account".into(),
            model_hint: "fixture-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (parent, branch) = store.create(&actor, "Parent", parent_binding).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "delegate".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-budget".into(), name: "subagent".into(),
            arguments: json!({"task":"workspace task"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-budget".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"workspace task"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Workspace,
                BudgetConfig::new(4, 1, 1, 1, 3600).unwrap(),
                fixture.state.clone(),
                Some(fixture.git.clone()),
            ),
        )
        .unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({"task":"workspace task","mode":"foreground"}),
            )
            .await
            .unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("delegation model-attempt limit reached"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let relation = store
            .subagents_for_parent(&actor, &parent.id)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(relation.state, "failed");
        let root = store
            .delegation_root_for_session(&actor, &relation.child_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(root.model_attempts_started, 1);
        assert_eq!(root.model_attempts_finished, 1);
        assert_eq!(root.active_children, 0);
        let execution = store
            .subagent_execution_binding_for_subagent(&actor, &relation.id)
            .unwrap()
            .unwrap();
        assert!(execution.execution_root.join("child-created.txt").exists());
        let cleanup = store
            .begin_subagent_execution_cleanup(&actor, &relation.id)
            .unwrap();
        let repo = aw_git::Repository::discover(&fixture.git, &fixture.repo).unwrap();
        repo.remove_managed_worktree(&cleanup.execution_root, true)
            .unwrap();
        store
            .finish_subagent_execution_cleanup(&actor, &cleanup.id)
            .unwrap();
    }

    #[tokio::test]
    async fn background_tool_returns_immediately_and_settles_child_independently() {
        let actor = PrincipalId::new("local:background").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let accounts = ProviderAccounts::new("local").unwrap();
        accounts
            .register(
                AccountDescriptor::personal(
                    "fixture-account",
                    actor.clone(),
                    "local",
                    ["fixture-model".into()],
                ),
                Arc::new(EchoProvider {
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        let identity = accounts
            .select(&actor, "fixture-account", "fixture-model")
            .unwrap()
            .identity()
            .clone();
        let store = SessionStore::in_memory().unwrap();
        let (parent, branch) = store.create(&actor, "Parent", binding()).unwrap();
        let (_, handle) = store.activate(&actor, &parent.id, &branch.id).unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "parent secret text".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        // The real Agent journals its Assistant proposal before the Process
        // tool intent; interrupted-parent fixtures preserve that ordering.
        let mut assistant = Message::text(2, Role::Assistant, "");
        assistant.tool_calls = vec![ToolCall {
            id: "call-subagent-background".into(), name: "subagent".into(),
            arguments: json!({"task":"research task","mode":"background"}),
        }];
        handle.append(1, JournalEntry::Assistant { message: assistant }).await.unwrap();
        handle
            .append(
                1,
                JournalEntry::ToolStarted {
                    record: ToolRecord {
                        call: ToolCall {
                            id: "call-subagent-background".into(),
                            name: "subagent".into(),
                            arguments: json!({"task":"research task","mode":"background"}),
                        },
                        effect: ToolEffect::Process,
                        execution_site: "local".into(),
                        status: ToolStatus::Running,
                        output: None,
                    },
                    source: None,
                },
            )
            .await
            .unwrap();
        let service = Service::new(
            store.clone(),
            accounts,
            settings(),
            ServiceConfig::new(
                ToolMode::Off,
                BudgetConfig::default(),
                PathBuf::from("/tmp"),
                None,
            ),
        ).unwrap();
        let output = service
            .execute(
                ToolContext {
                    actor: actor.clone(),
                    turn_id: 1,
                    execution_site: "local".into(),
                    cancellation: CancellationToken::new(),
                    identity,
                    scope: Some(InvocationScope {
                        session_id: parent.id.clone(),
                        branch_id: branch.id.clone(),
                    }),
                },
                &json!({"task":"research task","mode":"background"}),
            )
            .await
            .unwrap();
        let started: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(started["state"], "running");
        let child_session = started["session_id"].as_str().unwrap().to_owned();

        let mut settled = None;
        for _ in 0..100 {
            let relation = store
                .subagents_for_parent(&actor, &parent.id)
                .unwrap()
                .into_iter()
                .find(|item| item.child_session_id == child_session)
                .unwrap();
            if relation.state != "running" && relation.state != "admitted" {
                settled = Some(relation);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let relation = settled.expect("background child did not settle");
        assert_eq!(relation.state, "completed");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (view, _) = store.activate(&actor, &child_session, "main").unwrap();
        assert_eq!(view.history.turns.len(), 1);
        assert_eq!(view.history.turns[0].answer, "child answer");
        assert!(store.branches(&actor, &parent.id).unwrap()[0]
            .active_run
            .is_some());
    }

}