//! One headless execution path for CLI/web/embedded clients. No HTTP, TTY,
//! filesystem or subprocess implementation is in this crate.
use crate::{conversation::InvocationRecord, retry::RetryPolicy, *};
use aw_agent_kernel::{
    capability::Effect as PortableEffect,
    hook_contract::Stage as HookStage,
    turn::{Event as PortableEvent, Limits as PortableLimits, ProposedCall, TurnMachine, WaveKind},
};
use futures_util::{stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{sync::broadcast, time::Instant};

#[derive(Clone, Debug)]
pub struct AgentOptions {
    pub system_prompt: String,
    pub profile: String,
    pub model: String,
    pub request_timeout: Duration,
    pub hook_timeout: Duration,
    pub max_output_bytes: usize,
    pub retry: RetryPolicy,
}
impl AgentOptions {
    pub fn new(profile: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            system_prompt: String::new(),
            profile: profile.into(),
            model: model.into(),
            request_timeout: Duration::from_secs(600),
            hook_timeout: Duration::from_secs(15),
            max_output_bytes: crate::limits::MAX_OUTPUT_BYTES,
            retry: RetryPolicy::default(),
        }
    }
    fn validate(&self) -> Result<()> {
        for value in [&self.profile, &self.model] {
            if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(Error::Config(
                    "profile/model must be printable identifiers of 1..256 bytes".into(),
                ));
            }
        }
        if self.request_timeout.is_zero()
            || self.request_timeout > Duration::from_secs(86400)
            || self.hook_timeout.is_zero()
            || self.hook_timeout > Duration::from_secs(60)
        {
            return Err(Error::Config(
                "request/hook timeout is outside supported bounds".into(),
            ));
        }
        if self.max_output_bytes == 0 || self.max_output_bytes > crate::limits::MAX_OUTPUT_BYTES {
            return Err(Error::Config("output limit must be 1..524288 bytes".into()));
        }
        RetryPolicy::new(self.retry.pre_dispatch_retries)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Started {
        turn_id: u64,
        profile: String,
        model: String,
    },
    Delta {
        turn_id: u64,
        text: String,
    },
    ToolStarted {
        turn_id: u64,
        record: ToolRecord,
    },
    ToolFinished {
        turn_id: u64,
        record: ToolRecord,
    },
    InvocationFinished {
        turn_id: u64,
        record: InvocationRecord,
    },
    Finished {
        turn: Turn,
    },
    Rejected {
        message: String,
    },
}
pub struct AgentBuilder {
    provider: Arc<dyn Provider>,
    options: AgentOptions,
    extensions: Vec<Arc<dyn Extension>>,
    tools: Vec<Arc<dyn Tool>>,
    tool_policy: ToolPolicy,
    revision_policy: Option<Arc<dyn crate::tool::ToolRevisionPolicy>>,
    account: Option<(AccountDescriptor, PrincipalId, String)>,
    delegated_default: bool,
    selected_parent: Option<ProviderSelection>,
}
impl AgentBuilder {
    pub fn extension(mut self, extension: impl Extension + 'static) -> Self {
        self.extensions.push(Arc::new(extension));
        self
    }
    pub fn shared_extension(mut self, extension: Arc<dyn Extension>) -> Self {
        self.extensions.push(extension);
        self
    }
    pub fn tool(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }
    pub fn shared_tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }
    pub fn tool_policy(mut self, policy: ToolPolicy) -> Self {
        self.tool_policy = policy;
        self
    }
    /// Explicit trusted host authorization, never inferred from an external
    /// hook, model selection or project document. Default is *no revisions*.
    pub fn tool_revision_policy(
        mut self,
        policy: Arc<dyn crate::tool::ToolRevisionPolicy>,
    ) -> Self {
        self.revision_policy = Some(policy);
        self
    }
    /// Explicit host-owned account for the shorthand `prompt` path. Hosts with
    /// multiple users should select a lease per prompt and call `prompt_as`.
    pub fn account(
        mut self,
        account: AccountDescriptor,
        actor: PrincipalId,
        executor: impl Into<String>,
    ) -> Self {
        self.account = Some((account, actor, executor.into()));
        self.delegated_default = false;
        self.selected_parent = None;
        self
    }
    /// Explicit personal-device default. Registration still checks declared
    /// adapter/account/executor binding; ordinary account() is unchanged.
    pub fn delegated_account(
        mut self,
        account: AccountDescriptor,
        actor: PrincipalId,
        executor: impl Into<String>,
    ) -> Self {
        self.account = Some((account, actor, executor.into()));
        self.delegated_default = true;
        self.selected_parent = None;
        self
    }
    pub fn build(self) -> Result<Agent> {
        self.options.validate()?;
        self.tool_policy.validate()?;
        crate::extension::validate_extensions(&self.extensions)?;
        let mut tools = ToolRegistry::default();
        for t in self.tools {
            tools.register(t)?;
        }
        let (provider, pool, descriptor, actor) = if let Some(selected) = self.selected_parent {
            selected.revalidate()?;
            if self.options.profile != selected.identity().account_id
                || self.options.model != selected.identity().model
            {
                return Err(Error::Config(
                    "child Agent options must match the exact selected parent account/model".into(),
                ));
            }
            (
                selected.provider_arc(),
                selected.pool(),
                selected.descriptor()?,
                selected.identity().actor.clone(),
            )
        } else {
            let (descriptor, actor, executor) = match self.account {
                Some(a) => a,
                None => {
                    let actor = PrincipalId::new("local")?;
                    (
                        AccountDescriptor::personal(
                            self.options.profile.clone(),
                            actor.clone(),
                            "local",
                            [self.options.model.clone()],
                        ),
                        actor,
                        "local".into(),
                    )
                }
            };
            let pool = ProviderAccounts::new(executor)?;
            if self.delegated_default {
                pool.register_delegated(descriptor.clone(), self.provider.clone())?;
            } else {
                pool.register(descriptor.clone(), self.provider.clone())?;
            }
            pool.select(&actor, &descriptor.id, &self.options.model)?;
            (self.provider.clone(), pool, descriptor, actor)
        };
        Ok(Agent {
            conversation: Conversation::new(self.options.system_prompt.clone())?,
            provider,
            options: self.options,
            extensions: self.extensions,
            tools,
            tool_policy: self.tool_policy,
            revision_policy: self.revision_policy,
            default_pool: pool,
            default_account: descriptor,
            default_actor: actor,
            events: None,
            needs_reconciliation: false,
            journal: None,
            persistence_fault: false,
            last_portable_trace: Vec::new(),
            hook_observer_failures: AtomicU64::new(0),
        })
    }
}
/// Source-pinned immutable revision evidence: the caller cannot alter any
/// original Assistant proposal, declared effect or host-approved destination.
struct RevisionEvidence<'a> {
    original: &'a ToolCall,
    revised: &'a ToolCall,
    spec: &'a ToolSpec,
    source: &'a ToolSource,
    effect: &'a ToolEffect,
}
pub struct Agent {
    conversation: Conversation,
    provider: Arc<dyn Provider>,
    options: AgentOptions,
    extensions: Vec<Arc<dyn Extension>>,
    tools: ToolRegistry,
    tool_policy: ToolPolicy,
    revision_policy: Option<Arc<dyn crate::tool::ToolRevisionPolicy>>,
    default_pool: ProviderAccounts,
    default_account: AccountDescriptor,
    default_actor: PrincipalId,
    events: Option<broadcast::Sender<AgentEvent>>,
    needs_reconciliation: bool,
    journal: Option<Arc<dyn TurnJournal>>,
    persistence_fault: bool,
    // Ephemeral, data-free transition evidence from the common portable
    // kernel. Never persisted as authority, and no credentials are included.
    last_portable_trace: Vec<PortableEvent>,
    hook_observer_failures: AtomicU64,
}
impl Agent {
    pub fn builder(provider: Arc<dyn Provider>, options: AgentOptions) -> AgentBuilder {
        AgentBuilder {
            provider,
            options,
            extensions: Vec::new(),
            tools: Vec::new(),
            tool_policy: ToolPolicy::default(),
            revision_policy: None,
            account: None,
            delegated_default: false,
            selected_parent: None,
        }
    }

    /// Build a child Agent around the exact already-authorized parent
    /// provider/account/model selection. The child shares the registry so
    /// revocation remains live, but starts with isolated conversation state and
    /// no tools/extensions unless the embedding host explicitly adds them.
    pub fn builder_from_selection(
        selection: ProviderSelection,
        options: AgentOptions,
    ) -> AgentBuilder {
        AgentBuilder {
            provider: selection.provider_arc(),
            options,
            extensions: Vec::new(),
            tools: Vec::new(),
            tool_policy: ToolPolicy::default(),
            revision_policy: None,
            account: None,
            delegated_default: false,
            selected_parent: Some(selection),
        }
    }
    pub fn summary_executor(&self) -> Result<crate::SummaryExecutor> {
        crate::SummaryExecutor::new(self.extensions.clone(), self.options.hook_timeout)
    }
    #[cfg(feature = "routing")]
    pub fn routing_executor(&self) -> Result<crate::RoutingExecutor> {
        crate::RoutingExecutor::new(self.extensions.clone(), self.options.hook_timeout)
    }
    #[cfg(feature = "planner")]
    pub fn planner_executor(&self) -> Result<crate::PlannerExecutor> {
        crate::PlannerExecutor::new(self.extensions.clone(), self.options.hook_timeout)
    }
    #[cfg(feature = "reviewer")]
    pub fn review_executor(&self) -> Result<crate::ReviewExecutor> {
        crate::ReviewExecutor::new(self.extensions.clone(), self.options.hook_timeout)
    }
    pub fn turns(&self) -> &[Turn] {
        self.conversation.turns()
    }
    pub fn options(&self) -> &AgentOptions {
        &self.options
    }
    /// User-authorized safe-provider-attempt ceiling. Unlike retrying an
    /// uncertain HTTP error, BeforeDispatch explicitly attests no vendor
    /// bytes were emitted. The same exact account/model is revalidated.
    pub fn set_pre_dispatch_retries(&mut self, retries: u8) -> Result<()> {
        self.options.retry = RetryPolicy::new(retries)?;
        Ok(())
    }
    /// Explicit host resource ceiling. 16 by default; no read permission is
    /// created by this setting, even at the maximum supported 128.
    pub fn set_max_parallel_reads(&mut self, value: usize) -> Result<()> {
        if !(1..=aw_agent_kernel::turn::MAX_READ_WAVE).contains(&value) {
            return Err(Error::Config("parallel read ceiling must be 1..128".into()));
        }
        self.tool_policy.max_parallel_reads = value;
        Ok(())
    }
    /// The host permits concurrent TRUSTED Bash only when explicitly enabled.
    /// The model must additionally mark each command parallel=true; this does
    /// not change Process/Unknown journal and reconciliation semantics.
    pub fn set_parallel_bash(&mut self, enabled: bool, limit: usize) -> Result<()> {
        if !(1..=aw_agent_kernel::turn::MAX_READ_WAVE).contains(&limit) {
            return Err(Error::Config("parallel bash ceiling must be 1..128".into()));
        }
        self.tool_policy.allow_parallel_bash = enabled;
        self.tool_policy.max_parallel_bash = limit;
        Ok(())
    }
    /// The most recent actual hosted turn's portable transition decisions.
    /// For std/embedded parity inspection only; not a persisted replay plan.
    pub fn portable_trace(&self) -> &[PortableEvent] {
        &self.last_portable_trace
    }
    /// Diagnostics for optional read-only hooks. Failures can never change
    /// factual journal results or block mandatory lifecycle/cancellation.
    pub fn hook_observer_failures(&self) -> u64 {
        self.hook_observer_failures.load(Ordering::Relaxed)
    }
    pub fn actor(&self) -> &PrincipalId {
        &self.default_actor
    }
    /// Trusted embedding host only: registration is not exposed as a browser API.
    pub fn register_provider_account(
        &self,
        account: AccountDescriptor,
        provider: Arc<dyn Provider>,
    ) -> Result<()> {
        self.default_pool.register(account, provider)
    }
    pub fn provider_accounts(&self) -> ProviderAccounts {
        self.default_pool.clone()
    }
    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.tools.visible(&self.default_actor, &self.tool_policy)
    }
    /// Host-only registration preflight. This includes ungranted tools so an
    /// external adapter can reject collisions/capacity *before* publishing any
    /// part of a discovered source batch. It never confers a grant.
    pub fn registered_tool_names(&self) -> Vec<String> {
        self.tools.tools.keys().cloned().collect()
    }
    /// Host-only extension metadata. Remote manifest names do not authorize
    /// gates; approval is a separate host decision before registration.
    pub fn registered_extension_names(&self) -> Vec<String> {
        self.extensions
            .iter()
            .map(|extension| extension.name().to_owned())
            .collect()
    }
    /// Attach a trusted-host selected hook to the existing Agent. The same
    /// ordered, bounded hook gates apply to CLI full and JSONL minimal.
    pub fn register_host_extension(&mut self, extension: Arc<dyn Extension>) -> Result<()> {
        let mut next = self.extensions.clone();
        next.push(extension);
        crate::extension::validate_extensions(&next)?;
        self.extensions = next;
        Ok(())
    }

    /// Trusted embedding-host registration of one additional tool for this
    /// Agent's already-authenticated default actor. This never changes provider
    /// accounts, filesystem roots, or another actor's grants.
    pub fn register_host_tool(&mut self, tool: Arc<dyn Tool>) -> Result<()> {
        // Register once and grant the exact validated cached name; do not
        // re-read a potentially stateful third-party Tool::spec implementation.
        let name = self
            .tools
            .register_with_name(tool, crate::tool::ToolSource::trusted_host())?;
        let effect = self
            .tools
            .effect_of(&name)
            .ok_or(Error::Denied("registered trusted host tool effect missing"))?;
        self.tool_policy.permit_source(
            self.default_actor.clone(),
            &name,
            &crate::tool::ToolSource::trusted_host(),
            effect,
        )
    }
    /// Register a third-party tool without granting it to anyone. Its source
    /// epoch is host-minted; neither an MCP schema nor a plugin may claim
    /// ordinary trusted-host tool authority.
    pub fn register_source_tool(
        &mut self,
        tool: Arc<dyn Tool>,
        source: crate::tool::ToolSource,
    ) -> Result<()> {
        if source.kind == aw_agent_kernel::capability::SourceKind::Host {
            return Err(Error::Denied(
                "external registrations cannot impersonate a Host tool source",
            ));
        }
        self.tools.register_from(tool, source)
    }

    /// Grant the exact currently registered external tool source/effect.
    /// Host tools use the same predicate via trusted-host registration.
    pub fn permit_registered_source_tool(
        &mut self,
        actor: PrincipalId,
        name: &str,
        expected_source: &crate::tool::ToolSource,
    ) -> Result<()> {
        let source = self
            .tools
            .source_of(name)
            .ok_or(Error::Denied("tool not registered"))?;
        if source != expected_source || source.kind == aw_agent_kernel::capability::SourceKind::Host
        {
            return Err(Error::Denied(
                "registered tool origin/epoch does not match grant",
            ));
        }
        let effect = self
            .tools
            .effect_of(name)
            .ok_or(Error::Denied("tool effect missing"))?;
        self.tool_policy.permit_source(actor, name, source, effect)
    }

    /// Revocation removes future advertisements and authorization. In-flight
    /// effects still obey the existing journal's unknown-effect fencing.
    pub fn revoke_registered_source_tool(
        &mut self,
        actor: &PrincipalId,
        name: &str,
        source: &crate::tool::ToolSource,
    ) {
        self.tool_policy.revoke_source(actor, name, source);
    }

    pub fn tool_execution_site(&self) -> &str {
        &self.tool_policy.execution_site
    }
    pub fn default_identity(&self) -> Result<InvocationIdentity> {
        Ok(self
            .default_pool
            .select(
                &self.default_actor,
                &self.default_account.id,
                &self.options.model,
            )?
            .identity()
            .clone())
    }
    /// Memory-only reset. Persistent owners must create/switch a stored branch
    /// through their store and then load_history; reset never deletes history.
    pub fn reset(&mut self) -> Result<()> {
        if self.journal.is_some() {
            return Err(Error::Denied(
                "use the persistent session controller to create a new conversation",
            ));
        }
        self.conversation.clear();
        self.last_portable_trace.clear();
        Ok(())
    }
    pub fn history_state(&self) -> HistoryState {
        self.conversation.history_state(self.needs_reconciliation())
    }
    pub fn context_blocks(&self) -> &[crate::ContextBlock] {
        self.conversation.context_blocks()
    }

    /// Build the same public provider context used by Chat without admitting a
    /// conversation turn. Trusted hosts use this only with the journaled,
    /// tool-free AuxiliaryExecutor for side questions/generated metadata.
    pub async fn auxiliary_messages(
        &self,
        selected: &ProviderSelection,
        prompt: &str,
    ) -> Result<Vec<Message>> {
        if self.persistence_fault {
            return Err(Error::Storage(
                "journal failed; reload/recover the session before continuing",
            ));
        }
        if self.needs_reconciliation {
            return Err(Error::Denied(
                "interrupted tool effects require explicit host inspection/acknowledgement",
            ));
        }
        let mut selection_reader = selected.reader()?;
        selection_reader.revalidate()?;
        let identity = selected.identity();
        if let Some(journal) = &self.journal {
            journal.validate_context_as(identity).await?;
        }
        let mut messages = self.conversation.messages_for(prompt)?;
        if let Some(journal) = &self.journal {
            let current = messages
                .pop()
                .ok_or(Error::Protocol("missing auxiliary user prompt"))?;
            for block in journal.additional_context().await? {
                block.validate(0)?;
                messages.push(block.message(2 + messages.len() as u64));
            }
            messages.push(current);
        }
        for message in &mut messages {
            message.provider_context = None;
        }
        let context = HookContext {
            turn_id: 0,

            continuation: None,
            profile: &identity.account_id,
            model: &identity.model,
            identity,
        };
        messages = crate::hooks::dispatch_extension(
            &self.extensions,
            context,
            crate::extension::HookEvent::Prompt {
                input: prompt.to_owned(),
                messages,
            },
            self.options.hook_timeout,
            None,
        )
        .await?
        .prompt()?;
        validate_messages(&messages)?;
        selection_reader.revalidate()?;
        if let Some(journal) = &self.journal {
            journal.validate_context_as(identity).await?;
        }
        Ok(messages)
    }

    pub fn is_journaled(&self) -> bool {
        self.journal.is_some()
    }
    /// Source of truth for an already authenticated Session/branch binding.
    /// No new Session grant can be constructed from an Actor's display name.
    pub fn journal_scope(&self) -> Result<Option<crate::InvocationScope>> {
        self.journal
            .as_ref()
            .map_or(Ok(None), |journal| journal.invocation_scope())
    }
    /// A pending argument revision remains bound to the exact tool source,
    /// effect and independently approved host policy even after awaited
    /// hooks/journal steps. A recorded revision is not a reusable grant.
    fn revalidate_tool_revision(
        &self,
        evidence: RevisionEvidence<'_>,
        identity: &InvocationIdentity,
        gate: Option<&dyn crate::control::AgentStepGate>,
    ) -> Result<()> {
        let RevisionEvidence {
            original,
            revised,
            spec,
            source: expected_source,
            effect: expected_effect,
        } = evidence;
        let current = self
            .tools
            .source_of(&original.name)
            .ok_or(Error::Denied("tool source disappeared during revision"))?;
        if current != expected_source
            || &spec.effect != expected_effect
            || !self
                .tools
                .allows_bound(&identity.actor, &self.tool_policy, &original.name, gate)
        {
            return Err(Error::Denied("tool revision source/effect/grant revoked"));
        }
        let policy = self
            .revision_policy
            .as_ref()
            .ok_or(Error::Denied("tool revision has no host approval policy"))?;
        crate::tool::check_revision_policy(
            policy.as_ref(),
            original,
            revised,
            spec,
            expected_source,
            identity,
        )?;
        Ok(())
    }

    /// Data-only list of exact capabilities this Agent CURRENTLY exposes.
    /// It is NOT a grant: the owning parent Envelope must independently
    /// approve each exact source/effect before child binding, and ToolPolicy
    /// must revalidate at each actual effect dispatch.
    pub fn requested_tool_envelope_grants(
        &self,
    ) -> Result<Vec<aw_agent_kernel::envelope::ExactGrant>> {
        self.tools
            .visible(&self.default_actor, &self.tool_policy)
            .into_iter()
            .map(|spec| {
                let source = self
                    .tools
                    .source_of(&spec.name)
                    .ok_or(Error::Denied("visible child tool has no trusted source"))?;
                Ok(aw_agent_kernel::envelope::ExactGrant {
                    name: spec.name,
                    kind: source.kind,
                    source: source.id.clone(),
                    source_epoch: source.epoch,
                    effect: portable_effect(&spec.effect),
                })
            })
            .collect()
    }

    /// An Actor Envelope is a *ceiling* on the already explicit ToolPolicy,
    /// never a new tool grant. Check source kind/id/epoch, exact effect and
    /// execution site for every currently model-visible capability before a
    /// queued Turn can begin. Ordinary Agent calls retain their host policy.
    pub fn validate_runtime_envelope(
        &self,
        envelope: &aw_agent_kernel::envelope::Envelope,
    ) -> Result<()> {
        let budget = envelope.budget();
        if budget.max_model_rounds < self.tool_policy.max_model_rounds
            || budget.max_tool_calls < self.tool_policy.max_tool_calls
            || budget.max_output_bytes < self.options.max_output_bytes
            || envelope.actor() != self.default_actor.as_str()
            || envelope.site() != self.tool_policy.execution_site
        {
            return Err(Error::Denied(
                "Actor Envelope cannot cover Agent execution limits/identity",
            ));
        }
        for spec in self.tools.visible(&self.default_actor, &self.tool_policy) {
            let source = self
                .tools
                .source_of(&spec.name)
                .ok_or(Error::Denied("visible tool has no pinned source"))?;
            if !crate::tool::envelope_permits_tool(envelope, &spec.name, source, &spec.effect) {
                return Err(Error::Denied(
                    "Actor Envelope lacks exact visible tool grant",
                ));
            }
        }
        Ok(())
    }

    /// Trusted host supplies an already-authorized projection and journal. This
    /// never restores credentials, tool grants or provider instances from disk.
    pub fn load_history(
        &mut self,
        state: HistoryState,
        journal: Option<Arc<dyn TurnJournal>>,
    ) -> Result<()> {
        let restored = Conversation::from_history(self.options.system_prompt.clone(), &state)?;
        self.conversation = restored;
        self.last_portable_trace.clear();
        self.needs_reconciliation = state.needs_reconciliation;
        self.journal = journal;
        self.persistence_fault = false;
        Ok(())
    }
    pub async fn acknowledge_reconciliation(&mut self, note: impl Into<String>) -> Result<()> {
        if self.persistence_fault {
            return Err(Error::Storage(
                "restart/reload the owner after a journal failure before acknowledgement",
            ));
        }
        let note = note.into();
        if note.trim().is_empty() || note.len() > 4096 {
            return Err(Error::Config(
                "inspection acknowledgement needs 1..4096 bytes".into(),
            ));
        }
        if let Some(j) = &self.journal {
            j.acknowledge(note).await?;
        }
        self.needs_reconciliation = false;
        Ok(())
    }
    pub fn needs_reconciliation(&self) -> bool {
        self.needs_reconciliation || self.persistence_fault
    }
    /// Trusted host must actually inspect the workspace/effects before this explicit acknowledgement.
    pub fn acknowledge_interrupted_tools(&mut self) -> Result<()> {
        if self.journal.is_some() {
            return Err(Error::Denied(
                "persistent acknowledgement must be journaled; use acknowledge_reconciliation",
            ));
        }
        self.needs_reconciliation = false;
        Ok(())
    }
    pub fn retryable_prompt(&self) -> Option<&str> {
        if self.needs_reconciliation() {
            None
        } else {
            self.conversation.retryable_prompt()
        }
    }
    pub fn set_model(&mut self, model: impl Into<String>) -> Result<()> {
        let mut options = self.options.clone();
        options.model = model.into();
        options.validate()?;
        // Only for this configured single-user default. Explicit per-user leases
        // cannot alter each other's allowed-model set or retained preferences.
        if self.provider.delegated_binding().is_some() {
            self.default_pool.select(
                &self.default_actor,
                &self.default_account.id,
                &options.model,
            )?;
            self.options = options;
            return Ok(());
        }
        self.default_account
            .allowed_models
            .insert(options.model.clone());
        self.default_pool
            .register(self.default_account.clone(), self.provider.clone())?;
        self.options = options;
        Ok(())
    }
    pub fn subscribe(&mut self) -> broadcast::Receiver<AgentEvent> {
        self.events
            .get_or_insert_with(|| broadcast::channel(128).0)
            .subscribe()
    }
    pub async fn prompt<C, F>(
        &mut self,
        prompt: impl Into<String>,
        cancel: C,
        on_event: F,
    ) -> Result<Turn>
    where
        C: Future<Output = ()> + Send,
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        let selected = self.default_pool.select(
            &self.default_actor,
            &self.default_account.id,
            &self.options.model,
        )?;
        self.prompt_as(&selected, prompt, cancel, on_event).await
    }
    /// The host establishes the authenticated user and session read/append ACL,
    /// then selects THAT user's provider lease. History is shared; credentials,
    /// accounts, actor, payer, model and inference location are per call.
    /// Inference may run in a local host independent of the remote history UI.
    pub async fn prompt_as<C, F>(
        &mut self,
        selected: &ProviderSelection,
        prompt: impl Into<String>,
        cancel: C,
        on_event: F,
    ) -> Result<Turn>
    where
        C: Future<Output = ()> + Send,
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        self.prompt_as_inner(selected, prompt, cancel, on_event, None)
            .await
    }
    /// One Actor admission adapter over the SAME Agent turn loop. The optional
    /// trusted host step gate only interrupts or parks at existing safe phases;
    /// it cannot change tool grants, inject unjournaled context or run models.
    pub(crate) async fn prompt_with_step_gate<C, F>(
        &mut self,
        prompt: impl Into<String>,
        cancel: C,
        on_event: F,
        gate: &dyn crate::control::AgentStepGate,
    ) -> Result<Turn>
    where
        C: Future<Output = ()> + Send,
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        let selection = self.default_pool.select(
            &self.default_actor,
            &self.default_account.id,
            &self.options.model,
        )?;
        self.prompt_as_inner(&selection, prompt, cancel, on_event, Some(gate))
            .await
    }
    /// Hosted Actor controller over the existing explicitly selected model
    /// lease. This never clones the Agent loop or relaxes owner/account/route.
    pub(crate) async fn prompt_with_step_gate_as<C, F>(
        &mut self,
        selected: &ProviderSelection,
        prompt: impl Into<String>,
        cancel: C,
        on_event: F,
        gate: &dyn crate::control::AgentStepGate,
    ) -> Result<Turn>
    where
        C: Future<Output = ()> + Send,
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        selected.revalidate()?;
        self.prompt_as_inner(selected, prompt, cancel, on_event, Some(gate))
            .await
    }
    async fn prompt_as_inner<C, F>(
        &mut self,
        selected: &ProviderSelection,
        prompt: impl Into<String>,
        cancel: C,
        mut on_event: F,
        step_gate: Option<&dyn crate::control::AgentStepGate>,
    ) -> Result<Turn>
    where
        C: Future<Output = ()> + Send,
        F: FnMut(&AgentEvent) -> Result<()> + Send,
    {
        let mut selection_reader = selected.reader()?;
        let mut sink_reader = selected.reader()?;
        let prompt = prompt.into();
        let deadline = Instant::now() + self.options.request_timeout;
        tokio::pin!(cancel);
        let identity = selected.identity().clone();
        let id = self.conversation.next_turn_id();
        let ctx = HookContext {
            turn_id: id,

            continuation: None,
            profile: &identity.account_id,
            model: &identity.model,
            identity: &identity,
        };
        let prepare = async {
            if self.persistence_fault {
                return Err(Error::Storage(
                    "journal failed; reload/recover the session before continuing",
                ));
            }
            if self.needs_reconciliation {
                return Err(Error::Denied(
                    "interrupted tool effects require explicit host inspection/acknowledgement",
                ));
            }
            selection_reader.revalidate()?;
            if let Some(journal) = &self.journal {
                journal.validate_context_as(&identity).await?;
            }
            let mut messages = self.conversation.messages_for(&prompt)?;
            if let Some(j) = &self.journal {
                let current = messages
                    .pop()
                    .ok_or(Error::Protocol("missing user prompt"))?;
                for block in j.additional_context().await? {
                    block.validate(0)?;
                    messages.push(block.message(2 + messages.len() as u64));
                }
                messages.push(current);
                validate_messages(&messages)?;
            }
            // Provider-private thinking context is never silently sent under a
            // different model/account. Public transcript remains shared.
            for message in &mut messages {
                message.provider_context = None;
            }
            self.conversation
                .check_retained_budget(&prompt, self.options.max_output_bytes)?;
            messages = crate::hooks::dispatch_extension(
                &self.extensions,
                ctx,
                crate::extension::HookEvent::Prompt {
                    input: prompt.clone(),
                    messages,
                },
                self.options.hook_timeout,
                step_gate.and_then(|gate| gate.hook_continuation_port()),
            )
            .await?
            .prompt()?;
            validate_messages(&messages)?;
            let specs = self
                .tools
                .visible(&identity.actor, &self.tool_policy)
                .into_iter()
                .filter(|spec| {
                    self.tools.allows_bound(
                        &identity.actor,
                        &self.tool_policy,
                        &spec.name,
                        step_gate,
                    )
                })
                .collect::<Vec<_>>();
            let schema_bytes = serde_json::to_vec(&specs)
                .map_err(|_| Error::Protocol("invalid tool schema"))?
                .len();
            if schema_bytes > 128 * 1024 {
                return Err(Error::Limit("total tool definitions exceed 128 KiB"));
            }
            if !specs.is_empty() && !selected.provider().supports_tools() {
                return Err(Error::Protocol(
                    "selected provider does not support configured tool protocol",
                ));
            }
            Ok::<_, Error>((messages, specs))
        };
        let (mut messages, specs) = match controlled(prepare, cancel.as_mut(), deadline).await {
            Ok(v) => v,
            Err(e) => {
                let _ = notify(
                    &self.events,
                    &mut on_event,
                    AgentEvent::Rejected {
                        message: e.to_string(),
                    },
                );
                return Err(e);
            }
        };
        let guarded = match &self.journal {
            Some(j) => j.requires_output_gate().await?,
            None => false,
        };
        let mut original_callback = on_event;
        let mut held = Vec::<AgentEvent>::new();
        let sender = if guarded { None } else { self.events.clone() };
        let mut on_event = |event: &AgentEvent| -> Result<()> {
            if !guarded || matches!(event, AgentEvent::Started { .. }) {
                return original_callback(event);
            }
            if let AgentEvent::Delta { turn_id, text } = event {
                if let Some(AgentEvent::Delta {
                    turn_id: last,
                    text: previous,
                }) = held.last_mut()
                {
                    if last == turn_id {
                        previous.push_str(text);
                        return Ok(());
                    }
                }
            }
            if held.len() >= 1024 {
                return Err(Error::Limit("protected output event buffer exceeded"));
            }
            held.push(event.clone());
            Ok(())
        };
        if let Some(journal) = &self.journal {
            if let Err(e) = journal
                .begin(TurnStart {
                    turn_id: id,
                    prompt: prompt.clone(),
                    identity: identity.clone(),
                })
                .await
            {
                self.persistence_fault = matches!(e, Error::Storage(_));
                return Err(e);
            }
        }
        self.hook_observer_failures.fetch_add(
            crate::hooks::observe_extension(
                &self.extensions,
                ctx,
                HookStage::Turn,
                "turn_started",
                self.options.hook_timeout,
            )
            .await,
            Ordering::Relaxed,
        );
        let mut turn = Turn {
            id,
            prompt: prompt.clone(),
            answer: String::new(),
            model: identity.model.clone(),
            profile: identity.account_id.clone(),
            outcome: Outcome::Failed,
            meta: None,
            error: None,
            actor: Some(identity.actor.clone()),
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages: vec![Message::text(id * 1024, Role::User, &prompt)],
            needs_reconciliation: false,
        };
        // The same turn machine is driven by std Agent and no_std+alloc
        // embedding hosts. Native async transport/journals are only host ports.
        let mut machine = portable_step(TurnMachine::new(
            id,
            portable_step(PortableLimits::new(
                self.tool_policy.max_model_rounds,
                self.tool_policy.max_tool_calls,
            ))?,
        ))?;
        let mut portable_trace = Vec::<PortableEvent>::new();
        let mut provider_attempt_ordinal = 0u32;
        let execution = async {
            notify(
                &self.events,
                &mut on_event,
                AgentEvent::Started {
                    turn_id: id,
                    profile: identity.account_id.clone(),
                    model: identity.model.clone(),
                },
            )?;
            self.hook_observer_failures.fetch_add(
                crate::hooks::observe_extension(
                    &self.extensions,
                    ctx,
                    HookStage::Event,
                    "agent_started",
                    self.options.hook_timeout,
                )
                .await,
                Ordering::Relaxed,
            );
            for round in 0..self.tool_policy.max_model_rounds {
                if let Some(gate) = step_gate {
                    // Prior effects have already been factually receipted.
                    // Cancelled here means a settled interrupted Turn, never
                    // replay or a fabricated tool result.
                    controlled(
                        gate.checkpoint(id, round + 1, crate::control::AgentGatePoint::BeforeModel),
                        cancel.as_mut(),
                        deadline,
                    )
                    .await?;
                }
                portable_trace.push(portable_step(machine.begin_model())?);
                debug_assert_eq!(machine.rounds(), round + 1);
                selection_reader.revalidate()?;
                validate_messages(&messages)?;
                // A model ROUND has one context, but can have more than one
                // distinct journaled PROVIDER attempt only when an adapter
                // positively attests no request was ever dispatched. Generic
                // Transport/HTTP/Timeout/stream errors are never replayed.
                let mut invocation_context = crate::InvocationContext {
                    purpose: Default::default(),
                    scope: match &self.journal {
                        Some(j) => j.invocation_scope()?,
                        None => None,
                    },
                    turn_id: id,
                    ordinal: 0,
                    identity: identity.clone(),
                };
                let mut round_text = String::new();
                let mut extra_used = 0u8;
                let reply = loop {
                    if provider_attempt_ordinal >= crate::MAX_PROVIDER_ATTEMPTS_PER_TURN {
                        return Err(Error::Limit(
                            "bounded provider-attempt limit reached; no hidden retry",
                        ));
                    }
                    provider_attempt_ordinal += 1;
                    invocation_context.ordinal = provider_attempt_ordinal;
                    selection_reader.revalidate()?;
                    if let Some(j) = &self.journal {
                        j.append(
                            id,
                            JournalEntry::InvocationStarted {
                                ordinal: invocation_context.ordinal,
                                identity: identity.clone(),
                            },
                        )
                        .await?;
                    }
                    let text_before = round_text.len();
                    let mut sink_fault = None;
                    let generated = {
                        let mut sink = |text: &str| -> Result<()> {
                            if sink_fault.is_some() {
                                return Err(Error::Protocol("stream already rejected"));
                            }
                            let accepted = (|| {
                                sink_reader.revalidate()?;
                                if turn.answer.len().saturating_add(text.len())
                                    > self.options.max_output_bytes
                                {
                                    return Err(Error::Limit("response text limit reached"));
                                }
                                round_text.push_str(text);
                                turn.answer.push_str(text);
                                notify(
                                    &sender,
                                    &mut on_event,
                                    AgentEvent::Delta {
                                        turn_id: id,
                                        text: text.into(),
                                    },
                                )
                            })();
                            if let Err(error) = accepted {
                                sink_fault = Some(error);
                                return Err(Error::Protocol("stream callback rejected"));
                            }
                            Ok(())
                        };
                        controlled(
                            async {
                                selection_reader.revalidate()?;
                                if let Some(j) = &self.journal {
                                    j.validate_context_as(&identity).await?;
                                }
                                selection_reader.revalidate()?;
                                selected
                                    .provider()
                                    .invoke(
                                        &invocation_context,
                                        &messages,
                                        &identity.model,
                                        &specs,
                                        &mut sink,
                                    )
                                    .await
                            },
                            cancel.as_mut(),
                            deadline,
                        )
                        .await
                    };
                    let undisbursed = matches!(generated, Err(Error::BeforeDispatch(_)))
                        && round_text.len() == text_before
                        && sink_fault.is_none();
                    let record = InvocationRecord {
                        ordinal: invocation_context.ordinal,
                        identity: identity.clone(),
                        meta: generated.as_ref().ok().map(|r| r.meta.clone()),
                        outcome: if sink_fault.is_some() {
                            "stream_rejected"
                        } else if undisbursed {
                            "not_dispatched"
                        } else if generated.is_ok() {
                            "response_received"
                        } else {
                            "failed_or_cancelled"
                        }
                        .into(),
                        usage_verified: false,
                    };
                    turn.invocations.push(record.clone());
                    if let Some(j) = &self.journal {
                        j.append(
                            id,
                            JournalEntry::InvocationFinished {
                                record: record.clone(),
                            },
                        )
                        .await?;
                    }
                    self.hook_observer_failures.fetch_add(
                        crate::hooks::observe_extension(
                            &self.extensions,
                            ctx,
                            HookStage::Output,
                            "provider_attempt_recorded",
                            self.options.hook_timeout,
                        )
                        .await,
                        Ordering::Relaxed,
                    );
                    let _ = notify(
                        &sender,
                        &mut on_event,
                        AgentEvent::InvocationFinished {
                            turn_id: id,
                            record,
                        },
                    );
                    if let Some(error) = sink_fault {
                        return Err(error);
                    }
                    selection_reader.revalidate()?;
                    if let Err(ref error) = generated {
                        if self.options.retry.allow(
                            error,
                            round_text.len() != text_before,
                            extra_used,
                            provider_attempt_ordinal,
                        ) {
                            // The previous durable attempt has been fully
                            // settled above; a fresh unique ordinal makes any
                            // opted-in safe retry auditable, with no tool
                            // replay, different account/model or silent cost.
                            extra_used += 1;
                            continue;
                        }
                    }
                    break generated?;
                };
                turn.meta = Some(reply.meta.clone());
                let mut assistant = Message::text(
                    id * 1024 + turn.context_messages.len() as u64,
                    Role::Assistant,
                    round_text,
                );
                assistant.tool_calls = reply.tool_calls.clone();
                assistant.provider_context = reply.provider_context;
                if let Some(j) = &self.journal {
                    let mut recorded = assistant.clone();
                    recorded.provider_context = None;
                    j.append(id, JournalEntry::Assistant { message: recorded })
                        .await?;
                }
                self.hook_observer_failures.fetch_add(
                    crate::hooks::observe_extension(
                        &self.extensions,
                        ctx,
                        HookStage::Message,
                        "assistant_recorded",
                        self.options.hook_timeout,
                    )
                    .await,
                    Ordering::Relaxed,
                );
                if reply.tool_calls.is_empty() {
                    portable_trace.push(portable_step(
                        machine.complete_text(reply.meta.complete(), !turn.answer.is_empty()),
                    )?);
                    turn.context_messages.push(assistant);
                    if !reply.meta.complete() || turn.answer.is_empty() {
                        turn.outcome = Outcome::Incomplete;
                        turn.error = Some("provider did not complete a text answer".into());
                    } else {
                        turn.outcome = Outcome::Completed;
                    }
                    return Ok::<(), Error>(());
                }
                let proposals = reply
                    .tool_calls
                    .iter()
                    .map(|call| ProposedCall {
                        id: &call.id,
                        name: &call.name,
                    })
                    .collect::<Vec<_>>();
                let proposed_event = machine
                    .propose_batch(
                        &proposals,
                        matches!(reply.meta.finish_reason.as_str(), "tool_calls" | "stop"),
                    )
                    .map_err(|error| match error {
                        aw_agent_kernel::turn::Error::Exhausted => Error::Limit(
                            "tool-call budget exhausted; no calls from this round executed",
                        ),
                        aw_agent_kernel::turn::Error::DuplicateCall => {
                            Error::Protocol("duplicate tool-call ID; no replay")
                        }
                        aw_agent_kernel::turn::Error::InvalidCall => Error::Protocol(
                            "incomplete or invalid tool proposal; no calls executed",
                        ),
                        _ => Error::Protocol("portable tool batch admission rejected"),
                    })?;
                portable_trace.push(proposed_event);
                // Preflight the ENTIRE batch before any effect. Reject the
                // complete batch if one proposal is not permitted, malformed
                // or vetoed. Every original ID gets its own *no-effect*
                // denial record, so the model sees correctly paired errors
                // without a partial mutation or fabricated ToolStarted.
                let mut batch = Vec::new();
                let mut denied = std::collections::BTreeMap::<String, &'static str>::new();
                let mut revisions =
                    std::collections::BTreeMap::<String, (ToolCall, ToolSource, ToolEffect)>::new();
                for call in &reply.tool_calls {
                    call.validate_envelope()?;
                    if !self.tools.allows_bound(
                        &identity.actor,
                        &self.tool_policy,
                        &call.name,
                        step_gate,
                    ) {
                        denied.insert(call.id.clone(), "not granted or source unavailable");
                        continue;
                    }
                    let (tool, spec) = self.tools.get(&call.name).ok_or(Error::Protocol(
                        "registered tool disappeared before preflight",
                    ))?;
                    if tool.validate(&call.arguments).is_err() {
                        denied.insert(call.id.clone(), "invalid proposed tool arguments");
                        continue;
                    }
                    let approval = controlled(
                        crate::hooks::dispatch_extension(
                            &self.extensions,
                            ctx,
                            crate::extension::HookEvent::ToolCall {
                                call: call.clone(),
                                spec: spec.clone(),
                            },
                            self.options.hook_timeout,
                            step_gate.and_then(|gate| gate.hook_continuation_port()),
                        ),
                        cancel.as_mut(),
                        deadline,
                    )
                    .await;
                    let approved = match approval {
                        Ok(event) => match event.tool_call() {
                            Ok(call) => call,
                            Err(_) => {
                                denied.insert(call.id.clone(), "invalid hook proposal");
                                continue;
                            }
                        },
                        Err(Error::Cancelled) => return Err(Error::Cancelled),
                        Err(Error::Storage(_)) => {
                            return Err(Error::Storage(
                                "tool hook failed after a durable storage operation",
                            ))
                        }
                        Err(_) => {
                            denied.insert(call.id.clone(), "tool pre-effect hook denied");
                            continue;
                        }
                    };
                    approved.validate_envelope()?;
                    if approved.arguments != call.arguments {
                        let source = self
                            .tools
                            .source_of(&call.name)
                            .ok_or(Error::Denied("source disappeared before revision"))?;
                        let permitted = self.revision_policy.as_ref().is_some_and(|policy| {
                            crate::tool::check_revision_policy(
                                policy.as_ref(),
                                call,
                                &approved,
                                &spec,
                                source,
                                &identity,
                            )
                            .is_ok()
                        });
                        if !permitted {
                            denied
                                .insert(call.id.clone(), "host did not approve argument revision");
                            continue;
                        }
                        revisions.insert(
                            call.id.clone(),
                            (call.clone(), source.clone(), spec.effect.clone()),
                        );
                    }
                    if tool.validate(&approved.arguments).is_err()
                        || !self.tools.allows_bound(
                            &identity.actor,
                            &self.tool_policy,
                            &approved.name,
                            step_gate,
                        )
                    {
                        denied.insert(call.id.clone(), "grant/schema changed during tool hook");
                        continue;
                    }
                    batch.push((approved, tool, spec));
                }
                if !denied.is_empty() {
                    portable_trace.push(portable_step(machine.reject_batch())?);
                    // Assistant is an ORIGINAL immutable provider proposal.
                    // A denial cannot rewrite or delete its tool-call IDs.
                    messages.push(assistant.clone());
                    turn.context_messages.push(assistant);
                    for call in &reply.tool_calls {
                        let reason = denied
                            .get(&call.id)
                            .copied()
                            .unwrap_or("entire batch rejected before any effect");
                        let output =
                            ToolOutput::error(format!("ERROR: denied before execution ({reason})"));
                        let record = ToolRecord {
                            call: call.clone(),
                            effect: self
                                .tools
                                .effect_of(&call.name)
                                .cloned()
                                .unwrap_or(ToolEffect::Unknown),
                            execution_site: self.tool_policy.execution_site.clone(),
                            status: ToolStatus::Denied,
                            output: Some(output.clone()),
                        };
                        if let Some(j) = &self.journal {
                            controlled(
                                j.append(
                                    id,
                                    JournalEntry::ToolDenied {
                                        record: record.clone(),
                                    },
                                ),
                                cancel.as_mut(),
                                deadline,
                            )
                            .await?;
                        }
                        portable_trace.push(portable_step(machine.denial_recorded(&call.id))?);
                        turn.tool_records.push(record.clone());
                        let _ = notify(
                            &sender,
                            &mut on_event,
                            AgentEvent::ToolFinished {
                                turn_id: id,
                                record,
                            },
                        );
                        let message = tool_result_message(
                            id * 1024 + turn.context_messages.len() as u64,
                            call,
                            &output,
                        );
                        turn.context_messages.push(message.clone());
                        messages.push(message);
                    }
                    continue;
                }
                // All revisions are authorized by an independent trusted
                // host policy, and their exact original + revised proposals,
                // source epoch and effect commit BEFORE tool batch admission.
                // The provider-visible Assistant retains its original call.
                for (effective, _, spec) in &batch {
                    if let Some((original, source, effect)) = revisions.get(&effective.id) {
                        selection_reader.revalidate()?;
                        self.revalidate_tool_revision(
                            RevisionEvidence {
                                original,
                                revised: effective,
                                spec,
                                source,
                                effect,
                            },
                            &identity,
                            step_gate,
                        )?;
                        if let Some(j) = &self.journal {
                            controlled(
                                j.append(
                                    id,
                                    JournalEntry::ToolRevised {
                                        original: original.clone(),
                                        revised: effective.clone(),
                                        source_id: source.id.clone(),
                                        source_kind: format!("{:?}", source.kind).to_lowercase(),
                                        source_epoch: source.epoch,
                                        effect: effect.clone(),
                                    },
                                ),
                                cancel.as_mut(),
                                deadline,
                            )
                            .await?;
                        }
                    }
                }
                let proposed_bytes = assistant.context_bytes();
                // Reserve actual bounded capacity, rather than assuming every
                // result will fill its absolute 128KiB output allowance.
                // For large model-selected batches, apportion the available
                // model and trace context evenly. Each result is subsequently
                // truncated explicitly before it is journaled and disclosed.
                let context_used = messages
                    .iter()
                    .map(Message::context_bytes)
                    .sum::<usize>()
                    .saturating_add(proposed_bytes);
                let trace_used = turn
                    .context_messages
                    .iter()
                    .map(Message::context_bytes)
                    .sum::<usize>()
                    .saturating_add(proposed_bytes);
                const RESULT_OVERHEAD_BYTES: usize = 512;
                let per_call_capacity = limits::MAX_CONTEXT_BYTES
                    .saturating_sub(context_used)
                    .min(tool::MAX_TRACE_BYTES.saturating_sub(trace_used))
                    / batch.len();
                if per_call_capacity <= RESULT_OVERHEAD_BYTES {
                    return Err(Error::Limit(
                        "insufficient context budget for complete tool batch; no tools executed",
                    ));
                }
                let per_tool_output_bytes =
                    (per_call_capacity - RESULT_OVERHEAD_BYTES).min(tool::MAX_TOOL_RESULT_BYTES);
                if let Some(gate) = step_gate {
                    // The provider invocation and assistant answer are
                    // durable; no tool intent has been admitted yet.
                    // Pausing/urgent steer is safe HERE, never between an
                    // external effect's intent and factual receipt.
                    controlled(
                        gate.checkpoint(
                            id,
                            round + 1,
                            crate::control::AgentGatePoint::BeforeToolBatch,
                        ),
                        cancel.as_mut(),
                        deadline,
                    )
                    .await?;
                }
                portable_trace.push(portable_step(machine.admit_batch())?);
                messages.push(assistant.clone());
                turn.context_messages.push(assistant);
                // Consecutive host-classified exact Read effects can run in
                // bounded waves. Write/Process/Unknown are hard barriers.
                // No tokio::spawn: cancellation drops every in-flight future.
                let mut cursor = 0;
                while cursor < batch.len() {
                    // Model parallel:true must be explicit on each safe read.
                    // A sequential model choice becomes a wave barrier, but
                    // retains its actual Read effect for journal/reconciliation.
                    let effects = batch[cursor..]
                        .iter()
                        .map(|(call, _, spec)| {
                            if spec.effect == ToolEffect::Read
                                && call.arguments.get("parallel").and_then(|v| v.as_bool())
                                    == Some(true)
                            {
                                PortableEffect::Read
                            } else {
                                PortableEffect::Unknown
                            }
                        })
                        .collect::<Vec<_>>();
                    let read_size = aw_agent_kernel::turn::read_wave_len(
                        &effects,
                        self.tool_policy.max_parallel_reads,
                    );
                    // Only an explicitly granted trusted Bash and the user's
                    // opt-in can honor the model's parallel=true preference.
                    // Adjacent commands marked false/omitted are barriers.
                    let bash_size = if self.tool_policy.allow_parallel_bash {
                        batch[cursor..]
                            .iter()
                            .take(self.tool_policy.max_parallel_bash)
                            .take_while(|(call, _, spec)| {
                                spec.effect == ToolEffect::Process
                                    && call.name == "bash"
                                    && call.arguments.get("parallel").and_then(|v| v.as_bool())
                                        == Some(true)
                                    && self
                                        .tools
                                        .source_of(&call.name)
                                        .is_some_and(|source| *source == ToolSource::trusted_host())
                            })
                            .count()
                    } else {
                        0
                    };
                    let wave = if read_size >= 2 {
                        Some((WaveKind::Read, read_size))
                    } else if bash_size >= 2 {
                        Some((WaveKind::Bash, bash_size))
                    } else {
                        None
                    };
                    if let Some((wave_kind, wave_size)) = wave {
                        portable_trace
                            .push(portable_step(machine.begin_wave(wave_kind, wave_size))?);
                        let wave_start = turn.tool_records.len();
                        let wave = &batch[cursor..cursor + wave_size];

                        // Durable per-tool intent records are appended strictly
                        // in model proposal order before any wave effect starts.
                        // If admission fails, close only acknowledged intents:
                        // an append rejected by storage is not known committed.
                        let mut acknowledged = 0;
                        let launch = async {
                            for (call, _, spec) in wave {
                                selection_reader.revalidate()?;
                                let index = turn.tool_records.len();
                                turn.tool_records.push(ToolRecord {
                                    call: call.clone(),
                                    effect: spec.effect.clone(),
                                    execution_site: self.tool_policy.execution_site.clone(),
                                    status: ToolStatus::Running,
                                    output: None,
                                });
                                portable_trace.push(portable_step(machine.prepare_wave_intent(
                                    &call.id,
                                    portable_effect(&spec.effect),
                                ))?);
                                if let Some(j) = &self.journal {
                                    controlled(
                                        j.append(
                                            id,
                                            JournalEntry::ToolStarted {
                                                record: turn.tool_records[index].clone(),
                                                source: Some(crate::ToolSourceEvidence::from_source(
                                                    self.tools.source_of(&call.name).ok_or(
                                                        Error::Denied("Tool source missing before immutable intent"),
                                                    )?,
                                                )),
                                            },
                                        ),
                                        cancel.as_mut(),
                                        deadline,
                                    )
                                    .await?;
                                }
                                acknowledged += 1;
                                portable_trace.push(portable_step(machine.wave_intent_recorded())?);
                                notify(
                                    &sender,
                                    &mut on_event,
                                    AgentEvent::ToolStarted {
                                        turn_id: id,
                                        record: turn.tool_records[index].clone(),
                                    },
                                )?;
                            }
                            // Both the source lease and exact current registry
                            // grants are checked after all intents were flushed.
                            selection_reader.revalidate()?;
                            if let Some(j) = &self.journal {
                                j.validate_context_as(&identity).await?;
                            }
                            selection_reader.revalidate()?;
                            for (call, _, spec) in wave {
                                if let Some((original, source, effect)) = revisions.get(&call.id) {
                                    self.revalidate_tool_revision(
                                        RevisionEvidence {
                                            original,
                                            revised: call,
                                            spec,
                                            source,
                                            effect,
                                        },
                                        &identity,
                                        step_gate,
                                    )?;
                                }
                            }
                            let authorized = wave.iter().all(|(call, _, _)| {
                                self.tools.allows_bound(
                                    &identity.actor,
                                    &self.tool_policy,
                                    &call.name,
                                    step_gate,
                                )
                            });
                            portable_step(machine.start_wave(authorized))
                        }
                        .await;
                        match launch {
                            Ok(event) => portable_trace.push(event),
                            Err(error) => {
                                let closed = close_interrupted_reads(
                                    self.journal.as_ref(),
                                    &mut turn,
                                    wave_start,
                                    acknowledged,
                                )
                                .await?;
                                for record in closed {
                                    let _ = notify(
                                        &sender,
                                        &mut on_event,
                                        AgentEvent::ToolFinished {
                                            turn_id: id,
                                            record,
                                        },
                                    );
                                }
                                return Err(error);
                            }
                        }

                        let mut tokens = Vec::with_capacity(wave_size);
                        let jobs = FuturesUnordered::new();
                        for (ordinal, (call, tool, _)) in wave.iter().enumerate() {
                            let token = CancellationToken::new();
                            tokens.push(CancelOnDrop(token.clone()));
                            let call = call.clone();
                            let tool = tool.clone();
                            let journal = self.journal.clone();
                            let registry = &self.tools;
                            let policy = &self.tool_policy;
                            let identity = identity.clone();
                            let scope = invocation_context.scope.clone();
                            let revision = revisions.get(&call.id).cloned();
                            let revision_policy = self.revision_policy.clone();
                            jobs.push(async move {
                                let mut tool_reader = selected.reader()?;
                                // No async task may rely just on the gate above:
                                // each tool rechecks actor, site and source at
                                // its actual dispatch, after journal flush.
                                tool_reader.revalidate()?;
                                if let Some(j) = &journal {
                                    j.validate_context_as(&identity).await?;
                                }
                                tool_reader.revalidate()?;
                                if !registry.allows_bound(
                                    &identity.actor,
                                    policy,
                                    &call.name,
                                    step_gate,
                                ) {
                                    return Err(Error::Denied(
                                        "read tool no longer granted at dispatch",
                                    ));
                                }
                                if let Some((original, source, effect)) = revision.as_ref() {
                                    if registry.source_of(&call.name) != Some(source)
                                        || registry.effect_of(&call.name) != Some(effect)
                                    {
                                        return Err(Error::Denied("revised tool source changed"));
                                    }
                                    revision_policy
                                        .as_ref()
                                        .ok_or(Error::Denied("revision approval revoked"))?
                                        .authorize(
                                            original,
                                            &call,
                                            &registry
                                                .get(&call.name)
                                                .ok_or(Error::Denied("revised tool disappeared"))?
                                                .1,
                                            source,
                                            &identity,
                                        )?;
                                }
                                let output = tokio::time::timeout(
                                    policy.timeout,
                                    tool.execute(
                                        ToolContext {
                                            actor: identity.actor.clone(),
                                            turn_id: id,
                                            execution_site: policy.execution_site.clone(),
                                            cancellation: token,
                                            identity: identity.clone(),
                                            scope,
                                        },
                                        &call.arguments,
                                    ),
                                )
                                .await
                                .map_err(|_| Error::Timeout)??;
                                output.validate()?;
                                Ok::<_, Error>((
                                    ordinal,
                                    bounded_tool_output(output, per_tool_output_bytes),
                                ))
                            });
                        }
                        // FuturesUnordered polls concurrently without detached
                        // workers. On failure, timeout or cancellation, dropping
                        // this controlled future cancels all remaining reads.
                        let wave_output = controlled(
                            async move {
                                let mut ready = vec![None::<ToolOutput>; wave_size];
                                let mut jobs = jobs;
                                while let Some(result) = jobs.next().await {
                                    let (ordinal, output) = result?;
                                    ready[ordinal] = Some(output);
                                }
                                Ok(ready)
                            },
                            cancel.as_mut(),
                            deadline,
                        )
                        .await;
                        drop(tokens);
                        let wave_output = match wave_output {
                            Ok(outputs) => {
                                // Revocation during an in-flight read cannot
                                // leak stale content into the next model round.
                                let check = async {
                                    selection_reader.revalidate()?;
                                    if let Some(j) = &self.journal {
                                        j.validate_context_as(&identity).await?;
                                    }
                                    selection_reader.revalidate()?;
                                    for (call, _, spec) in wave {
                                        if let Some((original, source, effect)) =
                                            revisions.get(&call.id)
                                        {
                                            self.revalidate_tool_revision(
                                                RevisionEvidence {
                                                    original,
                                                    revised: call,
                                                    spec,
                                                    source,
                                                    effect,
                                                },
                                                &identity,
                                                step_gate,
                                            )?;
                                        }
                                    }
                                    if wave.iter().any(|(call, _, _)| {
                                        !self.tools.allows_bound(
                                            &identity.actor,
                                            &self.tool_policy,
                                            &call.name,
                                            step_gate,
                                        )
                                    }) {
                                        return Err(Error::Denied(
                                            "read wave source revoked during execution",
                                        ));
                                    }
                                    Ok::<(), Error>(())
                                }
                                .await;
                                check.map(|()| outputs)
                            }
                            failed => failed,
                        };
                        let outputs = match wave_output {
                            Ok(outputs) => outputs,
                            Err(error) => {
                                // An entire failed wave is interrupted, with
                                // every acknowledged intent closed in order.
                                // The model sees NONE of its partial results.
                                let closed = close_interrupted_reads(
                                    self.journal.as_ref(),
                                    &mut turn,
                                    wave_start,
                                    wave_size,
                                )
                                .await?;
                                for record in closed {
                                    let _ = notify(
                                        &sender,
                                        &mut on_event,
                                        AgentEvent::ToolFinished {
                                            turn_id: id,
                                            record,
                                        },
                                    );
                                }
                                return Err(error);
                            }
                        };
                        portable_trace.push(portable_step(machine.settle_wave())?);
                        // First seal every factual effect result in the durable
                        // journal in original order. A later hook veto or grant
                        // revocation cannot erase receipts for completed reads.
                        for (ordinal, output) in outputs.iter().enumerate() {
                            let output = output
                                .as_ref()
                                .ok_or(Error::Protocol("missing completed read wave result"))?;
                            let index = wave_start + ordinal;
                            turn.tool_records[index].status = if output.is_error {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Succeeded
                            };
                            turn.tool_records[index].output = Some(output.clone());
                            if let Some(j) = &self.journal {
                                controlled(
                                    j.append(
                                        id,
                                        JournalEntry::ToolFinished {
                                            record: turn.tool_records[index].clone(),
                                        },
                                    ),
                                    cancel.as_mut(),
                                    deadline,
                                )
                                .await?;
                            }
                            portable_trace.push(portable_step(machine.wave_receipt_recorded())?);
                        }
                        // Only after all receipts are persisted may ordered
                        // post-tool hooks and model-visible outputs be exposed.
                        // Revocation by a hook or connection invalidation blocks
                        // publication, but never deletes truthful past receipts.
                        for (ordinal, output) in outputs.iter().enumerate() {
                            let output = output
                                .as_ref()
                                .ok_or(Error::Protocol("missing completed read wave result"))?;
                            let (call, _, spec) = &wave[ordinal];
                            if let Some((original, source, effect)) = revisions.get(&call.id) {
                                self.revalidate_tool_revision(
                                    RevisionEvidence {
                                        original,
                                        revised: call,
                                        spec,
                                        source,
                                        effect,
                                    },
                                    &identity,
                                    step_gate,
                                )?;
                            }
                            let index = wave_start + ordinal;
                            let _ = notify(
                                &sender,
                                &mut on_event,
                                AgentEvent::ToolFinished {
                                    turn_id: id,
                                    record: turn.tool_records[index].clone(),
                                },
                            );
                            let public = controlled(
                                crate::hooks::dispatch_extension(
                                    &self.extensions,
                                    ctx,
                                    crate::extension::HookEvent::ToolResult {
                                        call: call.clone(),
                                        receipt: output.clone(),
                                        visible: output.clone(),
                                    },
                                    self.options.hook_timeout,
                                    step_gate.and_then(|gate| gate.hook_continuation_port()),
                                ),
                                cancel.as_mut(),
                                deadline,
                            )
                            .await?
                            .visible_result()?;
                            // Hook output is a MODEL-VISIBLE projection, not a
                            // new result quota. An extension cannot inflate a
                            // bounded, already-receipted result past the
                            // original per-call fair context share.
                            if public.content.len() > per_tool_output_bytes {
                                return Err(Error::Limit(
                                    "tool hook projection exceeded per-call context budget",
                                ));
                            }
                            selection_reader.revalidate()?;
                            if !self.tools.allows_bound(
                                &identity.actor,
                                &self.tool_policy,
                                &call.name,
                                step_gate,
                            ) {
                                return Err(Error::Denied(
                                    "read tool revoked before result publication",
                                ));
                            }
                            let message = tool_result_message(
                                id * 1024 + turn.context_messages.len() as u64,
                                call,
                                &public,
                            );
                            turn.context_messages.push(message.clone());
                            messages.push(message);
                        }
                        cursor += wave_size;
                        continue;
                    }
                    // Barrier or singleton path preserves the original
                    // exact journal/tool events and portable E2 trace.
                    let (call, tool, spec) = batch[cursor].clone();
                    cursor += 1;
                    selection_reader.revalidate()?; // account revocation also stops further paid/tool work
                    let index = turn.tool_records.len();
                    turn.tool_records.push(ToolRecord {
                        call: call.clone(),
                        effect: spec.effect.clone(),
                        execution_site: self.tool_policy.execution_site.clone(),
                        status: ToolStatus::Running,
                        output: None,
                    });
                    // Tool intent cannot be executed or skipped out of order.
                    portable_trace.push(portable_step(
                        machine.prepare_tool(&call.id, portable_effect(&spec.effect)),
                    )?);
                    if let Some(j) = &self.journal {
                        j.append(
                            id,
                            JournalEntry::ToolStarted {
                                record: turn.tool_records[index].clone(),
                                source: Some(crate::ToolSourceEvidence::from_source(
                                    self.tools.source_of(&call.name).ok_or(
                                        Error::Denied("Tool source missing before immutable intent"),
                                    )?,
                                )),
                            },
                        )
                        .await?;
                    }
                    portable_trace.push(portable_step(machine.intent_recorded())?);
                    notify(
                        &sender,
                        &mut on_event,
                        AgentEvent::ToolStarted {
                            turn_id: id,
                            record: turn.tool_records[index].clone(),
                        },
                    )?;
                    let token = CancellationToken::new();
                    let guard = CancelOnDrop(token.clone());
                    let output = controlled(
                        async {
                            // The intent and observer may both have yielded or
                            // revoked access. Recheck before entering tool code.
                            selection_reader.revalidate()?;
                            if let Some(j) = &self.journal {
                                j.validate_context_as(&identity).await?;
                            }
                            selection_reader.revalidate()?;
                            if let Some((original, source, effect)) = revisions.get(&call.id) {
                                self.revalidate_tool_revision(
                                    RevisionEvidence {
                                        original,
                                        revised: &call,
                                        spec: &spec,
                                        source,
                                        effect,
                                    },
                                    &identity,
                                    step_gate,
                                )?;
                            }
                            let authorized = self.tools.allows_bound(
                                &identity.actor,
                                &self.tool_policy,
                                &call.name,
                                step_gate,
                            );
                            if !authorized {
                                return Err(Error::Denied("tool no longer granted to this caller"));
                            }
                            portable_trace.push(portable_step(machine.start_effect(authorized))?);
                            tool.execute(
                                ToolContext {
                                    actor: identity.actor.clone(),
                                    turn_id: id,
                                    execution_site: self.tool_policy.execution_site.clone(),
                                    cancellation: token,
                                    identity: identity.clone(),
                                    scope: invocation_context.scope.clone(),
                                },
                                &call.arguments,
                            )
                            .await
                        },
                        cancel.as_mut(),
                        deadline.min(Instant::now() + self.tool_policy.timeout),
                    )
                    .await;
                    drop(guard);
                    match output {
                        Ok(output) => {
                            output.validate()?;
                            let output = bounded_tool_output(output, per_tool_output_bytes);
                            portable_trace.push(portable_step(machine.settle_effect())?);
                            turn.tool_records[index].status = if output.is_error {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Succeeded
                            };
                            turn.tool_records[index].output = Some(output.clone());
                            if let Some(j) = &self.journal {
                                j.append(
                                    id,
                                    JournalEntry::ToolFinished {
                                        record: turn.tool_records[index].clone(),
                                    },
                                )
                                .await?;
                            }
                            portable_trace.push(portable_step(machine.receipt_recorded())?);
                            let _ = notify(
                                &sender,
                                &mut on_event,
                                AgentEvent::ToolFinished {
                                    turn_id: id,
                                    record: turn.tool_records[index].clone(),
                                },
                            );
                            let public = controlled(
                                crate::hooks::dispatch_extension(
                                    &self.extensions,
                                    ctx,
                                    crate::extension::HookEvent::ToolResult {
                                        call: call.clone(),
                                        receipt: output.clone(),
                                        visible: output.clone(),
                                    },
                                    self.options.hook_timeout,
                                    step_gate.and_then(|gate| gate.hook_continuation_port()),
                                ),
                                cancel.as_mut(),
                                deadline,
                            )
                            .await?
                            .visible_result()?;
                            // Hook output is a MODEL-VISIBLE projection, not a
                            // new result quota. An extension cannot inflate a
                            // bounded, already-receipted result past the
                            // original per-call fair context share.
                            if public.content.len() > per_tool_output_bytes {
                                return Err(Error::Limit(
                                    "tool hook projection exceeded per-call context budget",
                                ));
                            }
                            selection_reader.revalidate()?;
                            if let Some((original, source, effect)) = revisions.get(&call.id) {
                                self.revalidate_tool_revision(
                                    RevisionEvidence {
                                        original,
                                        revised: &call,
                                        spec: &spec,
                                        source,
                                        effect,
                                    },
                                    &identity,
                                    step_gate,
                                )?;
                            }
                            if !self.tools.allows_bound(
                                &identity.actor,
                                &self.tool_policy,
                                &call.name,
                                step_gate,
                            ) {
                                return Err(Error::Denied(
                                    "tool grant revoked before result publication",
                                ));
                            }
                            let message = tool_result_message(
                                id * 1024 + turn.context_messages.len() as u64,
                                &call,
                                &public,
                            );
                            turn.context_messages.push(message.clone());
                            messages.push(message);
                        }
                        Err(e) => {
                            turn.tool_records[index].status = ToolStatus::Interrupted;
                            if let Some(j) = &self.journal {
                                j.append(
                                    id,
                                    JournalEntry::ToolFinished {
                                        record: turn.tool_records[index].clone(),
                                    },
                                )
                                .await?;
                            }
                            let _ = notify(
                                &sender,
                                &mut on_event,
                                AgentEvent::ToolFinished {
                                    turn_id: id,
                                    record: turn.tool_records[index].clone(),
                                },
                            );
                            return Err(e);
                        }
                    }
                }
                if turn
                    .context_messages
                    .iter()
                    .map(Message::context_bytes)
                    .sum::<usize>()
                    > tool::MAX_TRACE_BYTES
                {
                    return Err(Error::Limit("tool trace budget reached"));
                }
                if !turn.answer.is_empty() {
                    if turn.answer.len() + 2 > self.options.max_output_bytes {
                        return Err(Error::Limit("response text limit reached"));
                    }
                    turn.answer.push_str("\n\n");
                    notify(
                        &sender,
                        &mut on_event,
                        AgentEvent::Delta {
                            turn_id: id,
                            text: "\n\n".into(),
                        },
                    )?;
                }
            }
            Err(Error::Limit(
                "model/tool loop limit reached; no automatic continuation",
            ))
        }
        .await;
        if let Err(e) = execution {
            portable_trace.push(machine.fail());
            self.persistence_fault |= matches!(e, Error::Storage(_));
            turn.outcome = if matches!(e, Error::Cancelled) {
                Outcome::Cancelled
            } else {
                Outcome::Failed
            };
            turn.error = Some(e.to_string());
        }
        // Failed tool traces are retained but not silently replayed as complete
        // context. Even cancellation after a tool may have external side effects.
        for record in &mut turn.tool_records {
            if record.status == ToolStatus::Running {
                record.status = ToolStatus::Interrupted;
            }
        }
        turn.needs_reconciliation =
            turn.outcome != Outcome::Completed && machine.needs_reconciliation_if_failed();
        self.last_portable_trace = portable_trace;
        self.needs_reconciliation |= turn.needs_reconciliation;
        for message in &mut turn.context_messages {
            message.provider_context = None;
        }
        if let Some(j) = &self.journal {
            if let Err(e) = j.finish(turn.clone()).await {
                self.persistence_fault = true;
                self.needs_reconciliation = true;
                return Err(e);
            }
        }
        if guarded {
            if let Some(j) = &self.journal {
                if let Err(e) = j.validate_context().await {
                    self.persistence_fault = true;
                    return Err(e);
                }
            }
            for event in held {
                let _ = notify(&self.events, &mut original_callback, event);
            }
        }
        let turn = self.conversation.record(turn)?;
        self.hook_observer_failures.fetch_add(
            crate::hooks::observe_extension(
                &self.extensions,
                ctx,
                HookStage::Lifecycle,
                "turn_settled",
                self.options.hook_timeout,
            )
            .await,
            Ordering::Relaxed,
        );
        self.hook_observer_failures.fetch_add(
            crate::hooks::observe_extension(
                &self.extensions,
                ctx,
                HookStage::Turn,
                "turn_ended",
                self.options.hook_timeout,
            )
            .await,
            Ordering::Relaxed,
        );
        let _ = notify(
            &self.events,
            &mut original_callback,
            AgentEvent::Finished { turn: turn.clone() },
        );
        Ok(turn)
    }
}
/// Mark and journal every *acknowledged* read intent interrupted, in model
/// order. Called for failure before wave launch and cancellation/tool failure.
/// Never synthesize an effect result or retry an old read.
async fn close_interrupted_reads(
    journal: Option<&Arc<dyn TurnJournal>>,
    turn: &mut Turn,
    from: usize,
    count: usize,
) -> Result<Vec<ToolRecord>> {
    let mut records = Vec::with_capacity(count);
    for record in &mut turn.tool_records[from..from + count] {
        record.status = ToolStatus::Interrupted;
        record.output = None;
        let item = record.clone();
        if let Some(j) = journal {
            j.append(
                turn.id,
                JournalEntry::ToolFinished {
                    record: item.clone(),
                },
            )
            .await?;
        }
        records.push(item);
    }
    Ok(records)
}

/// Portable sequencing failures retain the native Agent error domain; they
/// never turn a denied effect into a silently successful host operation.
fn portable_step<T>(result: std::result::Result<T, aw_agent_kernel::turn::Error>) -> Result<T> {
    result.map_err(|error| match error {
        aw_agent_kernel::turn::Error::Exhausted => {
            Error::Limit("model/tool loop limit reached; no automatic continuation")
        }
        aw_agent_kernel::turn::Error::Capacity => {
            Error::Limit("portable Agent turn state capacity exceeded")
        }
        aw_agent_kernel::turn::Error::Denied => {
            Error::Denied("portable tool grant was revoked before execution")
        }
        _ => Error::Protocol("invalid portable Agent transition"),
    })
}
/// Explicitly bound one actual result to its fair share of this model batch.
/// The persisted receipt and model-visible projection both mark truncation.
fn bounded_tool_output(mut output: ToolOutput, budget: usize) -> ToolOutput {
    if output.content.len() > budget {
        let mut boundary = budget;
        while !output.content.is_char_boundary(boundary) {
            boundary -= 1;
        }
        output.content.truncate(boundary);
        output.truncated = true;
    }
    output
}

fn tool_result_message(id: u64, call: &ToolCall, output: &ToolOutput) -> Message {
    let mut message = Message::text(
        id,
        Role::Tool,
        format!(
            "{}{}{}",
            if output.is_error { "ERROR: " } else { "" },
            output.content,
            if output.truncated {
                "\n[output truncated]"
            } else {
                ""
            },
        ),
    );
    message.tool_call_id = Some(call.id.clone());
    message.name = Some(call.name.clone());
    message
}

fn portable_effect(effect: &ToolEffect) -> PortableEffect {
    match effect {
        ToolEffect::Read => PortableEffect::Read,
        ToolEffect::Write => PortableEffect::Write,
        ToolEffect::Process => PortableEffect::Process,
        ToolEffect::Unknown => PortableEffect::Unknown,
    }
}
struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
async fn controlled<T, F, C>(future: F, mut cancel: Pin<&mut C>, deadline: Instant) -> Result<T>
where
    F: Future<Output = Result<T>> + Send,
    C: Future<Output = ()> + Send,
{
    tokio::select! {biased; _=cancel.as_mut()=>Err(Error::Cancelled), result=tokio::time::timeout_at(deadline,future)=>result.map_err(|_|Error::Timeout)?}
}
fn notify<F>(
    sender: &Option<broadcast::Sender<AgentEvent>>,
    callback: &mut F,
    event: AgentEvent,
) -> Result<()>
where
    F: FnMut(&AgentEvent) -> Result<()>,
{
    if let Some(sender) = sender {
        let _ = sender.send(event.clone());
    }
    callback(&event)
}
