//! aw application facade. Rust library consumers should normally depend directly
//! on `aw-agent-core` (and, when needed, `aw-providers`), not on the CLI package.
use aw_agent_core::provider::{CompletionFuture, DeltaSink, Provider};
#[cfg(feature = "durable")]
use aw_agent_core::provider::{InvocationContext, ModelFuture};
use aw_agent_core::Message;
#[cfg(feature = "durable")]
use aw_agent_core::ToolSpec;
pub use aw_agent_core::{agent, completion, conversation, error, extension, limits};
pub use aw_agent_core::{
    AccountDescriptor, AccountOwner, BillingOwner, CredentialCustody, PrincipalId,
    ProviderAccounts, ProviderSelection, SourceKind, Tool, ToolPolicy, ToolSource,
};
pub use aw_agent_core::{
    Agent, AgentBuilder, AgentEvent, AgentOptions, CancellationToken, Error, Extension, Result,
};
pub use aw_providers::{catalog, config, discovery, provider};
#[cfg(feature = "durable")]
use aw_store::SessionStore;
pub use aw_tools::{LocalToolOptions, ToolMode};
use std::sync::Arc;
#[cfg(test)]
mod test_support;
#[cfg(feature = "durable")]
use zeroize::Zeroizing;
/// Native SQLite-writer-fenced Actor Input owner SDK. Does not open any RPC
/// listener or persist/replay queued message payloads on its own.
#[cfg(feature = "durable")]
pub mod actor_owner;
#[cfg(all(feature = "durable", feature = "pipelines", feature = "routing"))]
pub mod approval_ui;
pub mod context;
#[cfg(feature = "durable")]
pub mod context_flow;
pub mod extension_prompts;
#[cfg(feature = "durable")]
pub mod extension_secrets;
pub mod extension_settings;
pub mod extension_status;
pub mod headless;
#[cfg(feature = "jsonl")]
pub mod jsonl_compaction;
#[cfg(feature = "durable")]
pub mod model_agent_control;
#[cfg(feature = "durable")]
pub mod model_effects;
#[cfg(feature = "durable")]
pub mod model_management;
#[cfg(feature = "durable")]
pub mod model_process_control;
#[cfg(feature = "oidc")]
pub mod oidc_flow;
pub mod presentation;
pub mod project_resources;
#[cfg(feature = "durable")]
pub mod sessions;
pub mod terminal;
#[cfg(feature = "web")]
pub mod web;
#[cfg(feature = "surfaces")]
pub use aw_surfaces as surfaces;

struct UnconfiguredProvider;
impl Provider for UnconfiguredProvider {
    fn stream<'a>(
        &'a self,
        _messages: &'a [Message],
        _model: &'a str,
        _sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async {
            Err(Error::Config(
                "configure a provider/model in Settings before sending".into(),
            ))
        })
    }
}

#[cfg(feature = "durable")]
struct SecretStoreHttpProvider {
    profile: config::Profile,
    idle_timeout: std::time::Duration,
    store: SessionStore,
    actor: PrincipalId,
    account_id: String,
    expected_version: u64,
}

#[cfg(feature = "durable")]
impl SecretStoreHttpProvider {
    fn resolve(&self, model: &str) -> Result<provider::HttpProvider> {
        let secrets = self.store.local_secret_store()?;
        let resolved = self.store.resolve_server_provider_secret(
            &self.actor,
            &self.account_id,
            model,
            &self.profile.base_url,
            "local",
            self.expected_version,
            &secrets,
        )?;
        let expected_kind = match self.profile.kind {
            config::ProviderKind::Openai | config::ProviderKind::OpenaiResponses => "openai",
            config::ProviderKind::Ollama => "ollama",
            config::ProviderKind::Anthropic => "anthropic",
            config::ProviderKind::Gemini => "gemini",
        };
        if resolved.account.provider_kind != expected_kind {
            return Err(Error::Denied(
                "selected provider account kind does not match the selected profile",
            ));
        }
        let bearer = std::str::from_utf8(resolved.secret.as_slice()).map_err(|_| {
            Error::Config("selected server credential must be a UTF-8 bearer token".into())
        })?;
        let mut profile = self.profile.clone();
        profile.api_key_env = None;
        provider::HttpProvider::new_with_bearer(
            profile,
            Zeroizing::new(bearer.to_owned()),
            self.idle_timeout,
        )
    }
}

#[cfg(feature = "durable")]
impl Provider for SecretStoreHttpProvider {
    fn supports_tools(&self) -> bool {
        true
    }

    fn invoke<'a>(
        &'a self,
        context: &'a InvocationContext,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        let provider = if context.identity.account_id != self.account_id
            || context.identity.credential_version != Some(self.expected_version)
            || context.identity.actor != self.actor
        {
            Err(Error::Denied(
                "provider invocation identity does not match the selected SecretStore account/version",
            ))
        } else {
            self.resolve(model)
        };
        Box::pin(async move {
            let provider = provider?;
            provider.invoke(context, messages, model, tools, sink).await
        })
    }

    fn stream_tools<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        let provider = self.resolve(model);
        Box::pin(async move {
            let provider = provider?;
            provider.stream_tools(messages, model, tools, sink).await
        })
    }

    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        let provider = self.resolve(model);
        Box::pin(async move {
            let provider = provider?;
            provider.stream(messages, model, sink).await
        })
    }
}

#[cfg(feature = "oidc")]
struct OAuthStoreHttpProvider {
    profile: config::Profile,
    idle_timeout: std::time::Duration,
    store: SessionStore,
    actor: PrincipalId,
    connector_id: String,
    refresh_lock: tokio::sync::Mutex<()>,
}

#[cfg(feature = "oidc")]
impl OAuthStoreHttpProvider {
    async fn resolve(&self) -> Result<provider::HttpProvider> {
        if self.profile.kind != config::ProviderKind::OpenaiResponses {
            return Err(Error::Denied(
                "OAuth-backed ChatGPT plan inference requires the Responses adapter",
            ));
        }
        // Refresh tokens rotate. Serialize access-token refresh for one
        // registered ChatGPT account so concurrent callers cannot race and
        // invalidate each other's replacement refresh token.
        let _refresh_guard = self.refresh_lock.lock().await;
        let token =
            oidc_flow::chatgpt_plan_access_token(&self.store, &self.actor, &self.connector_id)
                .await?;
        let mut profile = self.profile.clone();
        profile.api_key_env = None;
        // The ChatGPT-plan direct route currently accepts only the stateless
        // Responses subset. Keep profile tuning metadata from leaking into
        // unsupported request fields on this credential path.
        profile.max_tokens = None;
        profile.temperature = None;
        provider::HttpProvider::new_with_bearer(profile, token, self.idle_timeout)
    }
}

#[cfg(feature = "oidc")]
impl Provider for OAuthStoreHttpProvider {
    fn supports_tools(&self) -> bool {
        true
    }

    fn invoke<'a>(
        &'a self,
        _context: &'a InvocationContext,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            let provider = self.resolve().await?;
            provider.stream_tools(messages, model, tools, sink).await
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
            let provider = self.resolve().await?;
            provider.stream_tools(messages, model, tools, sink).await
        })
    }

    fn stream<'a>(
        &'a self,
        messages: &'a [Message],
        model: &'a str,
        sink: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async move {
            let provider = self.resolve().await?;
            provider.stream(messages, model, sink).await
        })
    }
}

/// Build a same-user local execution for a SecretStore-backed Sign in with
/// ChatGPT connection. Construction reads only local metadata; OAuth refresh or
/// inference network I/O is deferred until the first admitted request.
#[cfg(feature = "oidc")]
pub fn configured_oauth_execution(
    settings: &config::Settings,
    _ctx: &context::ExecutionContext,
    tool_options: &LocalToolOptions,
    store: SessionStore,
    web_owner: bool,
) -> Result<Agent> {
    let connector_id = settings
        .profile
        .oauth_connector
        .clone()
        .ok_or_else(|| Error::Config("OAuth-backed profile has no connector".into()))?;
    if settings.profile.kind != config::ProviderKind::OpenaiResponses {
        return Err(Error::Config(
            "OAuth-backed ChatGPT profile must use openai-responses".into(),
        ));
    }
    let model = settings
        .profile
        .model
        .clone()
        .ok_or_else(|| Error::Config("model missing".into()))?;
    let actor = if web_owner {
        PrincipalId::new("browser:paired")?
    } else {
        PrincipalId::new(aw_platform::local_principal()?)?
    };
    let connector = store.oauth_connector_for_owner(&actor, &connector_id)?;
    if connector.status != "active" {
        return Err(Error::Denied("selected ChatGPT connection is not active"));
    }
    let provider: Arc<dyn Provider> = Arc::new(OAuthStoreHttpProvider {
        profile: settings.profile.clone(),
        idle_timeout: settings.idle_timeout,
        store,
        actor: actor.clone(),
        connector_id,
        refresh_lock: tokio::sync::Mutex::new(()),
    });
    let mut options = AgentOptions::new(settings.profile_name.clone(), model.clone());
    options.system_prompt = settings.system_prompt.clone();
    options.request_timeout = settings.request_timeout;
    options.max_output_bytes = settings.max_output_bytes;
    let account = AccountDescriptor::personal(
        settings.profile_name.clone(),
        actor.clone(),
        "local",
        settings.profile.effective_models(),
    );
    let builder = Agent::builder(provider, options).account(account, actor.clone(), "local");
    aw_tools::configure(builder, actor, tool_options)?.build()
}

/// Register one explicitly selected durable server/shared provider account into
/// this agent's transient registry for the current authenticated actor only.
/// Returns false when no durable account with this ID exists.
#[cfg(feature = "durable")]
pub fn register_persisted_server_account(
    agent: &Agent,
    settings: &config::Settings,
    store: SessionStore,
    account_id: &str,
) -> Result<bool> {
    if !store.provider_account_registered(account_id)? {
        return Ok(false);
    }
    let model = settings
        .profile
        .model
        .as_deref()
        .ok_or_else(|| Error::Config("model missing".into()))?;
    let info = store.provider_account_for_use(
        agent.actor(),
        account_id,
        model,
        &settings.profile.base_url,
        "local",
    )?;
    let version = info.secret_version.ok_or(Error::Denied(
        "selected server provider account has no active credential version",
    ))?;
    let expected_kind = match settings.profile.kind {
        config::ProviderKind::Openai | config::ProviderKind::OpenaiResponses => "openai",
        config::ProviderKind::Ollama => "ollama",
        config::ProviderKind::Anthropic => "anthropic",
        config::ProviderKind::Gemini => "gemini",
    };
    if info.provider_kind != expected_kind {
        return Err(Error::Denied(
            "selected provider account kind does not match the selected profile",
        ));
    }
    let provider: Arc<dyn Provider> = Arc::new(SecretStoreHttpProvider {
        profile: settings.profile.clone(),
        idle_timeout: settings.idle_timeout,
        store,
        actor: agent.actor().clone(),
        account_id: info.id.clone(),
        expected_version: version,
    });
    agent.register_provider_account(
        AccountDescriptor {
            id: info.id,
            owner: AccountOwner::Server(info.account_owner),
            payer: BillingOwner::Shared(info.payer),
            credential_custody: CredentialCustody::Server(info.executor_id.clone()),
            credential_version: Some(version),
            inference_executor: info.executor_id,
            allowed_models: info.allowed_models,
            grants: std::collections::BTreeSet::from([agent.actor().clone()]),
            shared_with_all_authenticated: false,
        },
        provider,
    )?;
    Ok(true)
}

/// Build the local control plane before any provider/model exists. This is used
/// only for the same-user loopback web UI so Settings can be opened first. The
/// placeholder account is revoked before returning and can never be selected.
pub fn unconfigured_execution(
    settings: &config::Settings,
    _ctx: &context::ExecutionContext,
    tool_options: &LocalToolOptions,
) -> Result<Agent> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    let mut options = AgentOptions::new(settings.profile_name.clone(), "__unconfigured");
    options.system_prompt = settings.system_prompt.clone();
    options.request_timeout = settings.request_timeout;
    options.max_output_bytes = settings.max_output_bytes;
    let account = AccountDescriptor::personal(
        "__unconfigured",
        actor.clone(),
        "local",
        ["__unconfigured".into()],
    );
    let builder = Agent::builder(Arc::new(UnconfiguredProvider), options).account(
        account,
        actor.clone(),
        "local",
    );
    let agent = aw_tools::configure(builder, actor, tool_options)?.build()?;
    agent.provider_accounts().revoke("__unconfigured")?;
    Ok(agent)
}

/// Builds the default SDK host without contacting a provider. Both frontends
/// call this factory. Custom consumers can register extensions on AgentBuilder.
pub fn configured_agent_builder(settings: &config::Settings) -> Result<AgentBuilder> {
    let model = settings.profile.model.clone().ok_or_else(|| {
        Error::Config("set --model-id / AW_MODEL / profile.model explicitly".into())
    })?;
    let provider = std::sync::Arc::new(provider::LazyHttpProvider::new(
        settings.profile.clone(),
        settings.idle_timeout,
    )?);
    let mut options = AgentOptions::new(settings.profile_name.clone(), model);
    options.system_prompt = settings.system_prompt.clone();
    options.request_timeout = settings.request_timeout;
    options.max_output_bytes = settings.max_output_bytes;
    Ok(Agent::builder(provider, options))
}

/// Default composition with no extra native extension registrations.
pub fn configured_agent(settings: &config::Settings) -> Result<Agent> {
    configured_agent_builder(settings)?.build()
}

/// Explicit native CLI or paired-browser composition. The browser principal is
/// not the server's Unix login; using a server account is an explicit owner grant.
pub fn configured_execution(
    settings: &config::Settings,
    _ctx: &context::ExecutionContext,
    tool_options: &LocalToolOptions,
    web_owner: bool,
) -> Result<Agent> {
    use std::collections::BTreeSet;
    let mut builder = configured_agent_builder(settings)?;
    let actor = if web_owner {
        PrincipalId::new("browser:paired")?
    } else {
        PrincipalId::new(aw_platform::local_principal()?)?
    };
    let model = settings
        .profile
        .model
        .clone()
        .ok_or_else(|| Error::Config("model missing".into()))?;
    let account = if web_owner {
        AccountDescriptor {
            id: settings.profile_name.clone(),
            owner: AccountOwner::Server("local-web-owner".into()),
            payer: BillingOwner::Shared("server-default".into()),
            credential_custody: CredentialCustody::Server("local".into()),
            credential_version: None,
            inference_executor: "local".into(),
            allowed_models: BTreeSet::from([model]),
            grants: BTreeSet::from([actor.clone()]),
            shared_with_all_authenticated: false,
        }
    } else {
        AccountDescriptor::personal(
            settings.profile_name.clone(),
            actor.clone(),
            "local",
            [model],
        )
    };
    builder = builder.account(account, actor.clone(), "local");
    aw_tools::configure(builder, actor, tool_options)?.build()
}

/// Register or replace one same-user OAuth-backed ChatGPT plan profile in the
/// live account registry. Token resolution/refresh remains lazy until dispatch.
#[cfg(feature = "oidc")]
pub fn register_oauth_profile(
    agent: &Agent,
    name: &str,
    profile: &config::Profile,
    idle_timeout: std::time::Duration,
    store: SessionStore,
) -> Result<bool> {
    let connector_id = profile
        .oauth_connector
        .clone()
        .ok_or_else(|| Error::Config("OAuth-backed profile has no connector".into()))?;
    if profile.kind != config::ProviderKind::OpenaiResponses {
        return Err(Error::Config(
            "OAuth-backed ChatGPT profile must use openai-responses".into(),
        ));
    }
    let models = profile.effective_models();
    if models.is_empty() {
        agent.provider_accounts().revoke(name)?;
        return Ok(false);
    }
    let connector = store.oauth_connector_for_owner(agent.actor(), &connector_id)?;
    if connector.status != "active" {
        return Err(Error::Denied("selected ChatGPT connection is not active"));
    }
    let provider: Arc<dyn Provider> = Arc::new(OAuthStoreHttpProvider {
        profile: profile.clone(),
        idle_timeout,
        store,
        actor: agent.actor().clone(),
        connector_id,
        refresh_lock: tokio::sync::Mutex::new(()),
    });
    agent.register_provider_account(
        AccountDescriptor::personal(name, agent.actor().clone(), "local", models),
        provider,
    )?;
    Ok(true)
}

/// Register or replace one local same-user provider profile in the live account
/// registry. A profile with no allowed models is deliberately not executable.
pub fn register_local_profile(
    agent: &Agent,
    name: &str,
    profile: &config::Profile,
    idle_timeout: std::time::Duration,
) -> Result<bool> {
    if profile.oauth_connector.is_some() {
        return Err(Error::Config(
            "OAuth-backed profiles require register_oauth_profile".into(),
        ));
    }
    let models = profile.effective_models();
    if models.is_empty() {
        agent.provider_accounts().revoke(name)?;
        return Ok(false);
    }
    let provider: Arc<dyn Provider> = Arc::new(provider::LazyHttpProvider::new(
        profile.clone(),
        idle_timeout,
    )?);
    agent.register_provider_account(
        AccountDescriptor::personal(name, agent.actor().clone(), "local", models),
        provider,
    )?;
    Ok(true)
}

/// Optional presentation data only. Prefer depending on aw-workspace directly
/// when embedding the reducer/editor without application/provider machinery.
#[cfg(feature = "workspace")]
pub use aw_workspace as workspace_model;

#[cfg(feature = "workspace")]
pub use aw_git as git;
#[cfg(feature = "workspace")]
pub use aw_terminal as terminal_service;

/// Optional local-owner client; has no provider configuration or state database.
#[cfg(feature = "client")]
pub use aw_client as client;

/// Optional personal-executor bridge. No transport is started by importing it.
#[cfg(feature = "device")]
pub use aw_inference as inference;

/// Optional stable internal participant directory; no authentication by naming.
#[cfg(feature = "team")]
pub use aw_identity as identity;
