use crate::sessions::PersistentSession;
use aw::{config, Agent, Error, ProviderSelection, Result};
use aw_agent_core::RoutingInput;
use aw_routing::{candidate_options, target, RoutingEvidence, RoutingMode, RoutingTargetRef};
use aw_store::{RoutingOperationInfo, RoutingSelectionSource};
use std::{collections::BTreeMap, future::Future};

#[derive(Clone, Copy)]
pub(crate) struct RoutingEnvironment<'a> {
    pub file_config: &'a config::FileConfig,
    pub idle_timeout: std::time::Duration,
    pub request_timeout: std::time::Duration,
}

#[derive(Clone, Debug)]
pub(crate) struct RoutingOptions {
    pub selector: Option<String>,
    pub revision: Option<String>,
    pub role: String,
    pub lineage: String,
}
impl Default for RoutingOptions {
    fn default() -> Self {
        Self {
            selector: None,
            revision: None,
            role: "main".into(),
            lineage: "main".into(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RouteDecision {
    pub selection: ProviderSelection,
    pub operation: RoutingOperationInfo,
}

fn profiles(file: &config::FileConfig) -> BTreeMap<String, config::Profile> {
    let mut profiles = config::builtins();
    profiles.extend(file.profiles.clone());
    profiles
}

fn target_profile<'a>(
    profiles: &'a BTreeMap<String, config::Profile>,
    target: &RoutingTargetRef,
) -> Result<&'a config::Profile> {
    let profile = profiles.get(&target.account_id).ok_or_else(|| {
        Error::Config(format!(
            "routing target {} references unknown saved profile {}",
            target.id, target.account_id
        ))
    })?;
    if !profile
        .effective_models()
        .iter()
        .any(|model| model == &target.model)
    {
        return Err(Error::Denied(
            "routing target model is no longer explicitly allowed by its saved profile",
        ));
    }
    let current = crate::sessions::destination_fingerprint(profile)?;
    if current != target.destination_fingerprint {
        return Err(Error::Conflict(
            "routing target destination changed since this immutable strategy revision; define a new revision before disclosure",
        ));
    }
    Ok(profile)
}

pub(crate) fn register_target(
    agent: &Agent,
    _store: &aw_store::SessionStore,
    profiles: &BTreeMap<String, config::Profile>,
    target: &RoutingTargetRef,
    idle_timeout: std::time::Duration,
) -> Result<ProviderSelection> {
    let profile = target_profile(profiles, target)?;
    if profile.oauth_connector.is_some() {
        #[cfg(feature = "oidc")]
        {
            aw::register_oauth_profile(
                agent,
                &target.account_id,
                profile,
                idle_timeout,
                _store.clone(),
            )?;
        }
        #[cfg(not(feature = "oidc"))]
        {
            return Err(Error::Config(
                "this build cannot resolve the OAuth-backed routing target".into(),
            ));
        }
    } else {
        aw::register_local_profile(agent, &target.account_id, profile, idle_timeout)?;
    }
    agent
        .provider_accounts()
        .select(agent.actor(), &target.account_id, &target.model)
}

pub(crate) fn preflight_strategy(
    file: &config::FileConfig,
    strategy: &aw_routing::RoutingStrategyDefinition,
) -> Result<BTreeMap<String, config::Profile>> {
    let profiles = profiles(file);
    for candidate in strategy
        .candidates
        .iter()
        .chain(strategy.classifier.iter())
    {
        let _ = target_profile(&profiles, candidate)?;
    }
    Ok(profiles)
}

pub(crate) fn routing_input(
    revision_id: &str,
    strategy: &aw_routing::RoutingStrategyDefinition,
    role: &str,
    lineage: &str,
    prompt: &str,
    timeout: std::time::Duration,
) -> Result<RoutingInput> {
    let input = RoutingInput {
        evidence: RoutingEvidence {
            strategy_revision_id: revision_id.into(),
            role: role.into(),
            lineage: lineage.into(),
            prompt: prompt.into(),
            candidates: candidate_options(strategy)
                .map_err(|error| Error::Config(error.to_string()))?,
        },
        timeout_seconds: timeout.as_secs().clamp(1, 600),
    };
    input.validate()?;
    Ok(input)
}

pub(crate) async fn select_main<C>(
    agent: &Agent,
    persistent: &PersistentSession,
    environment: RoutingEnvironment<'_>,
    options: &RoutingOptions,
    prompt: &str,
    cancel: C,
) -> Result<Option<RouteDecision>>
where
    C: Future<Output = ()> + Send,
{
    if options.role != "main" {
        return Err(Error::Config(
            "Phase 40 routes only the main Agent role; child roles are a later phase".into(),
        ));
    }
    let branch = persistent.handle.branch()?;
    let Some(resolved) = persistent.store.routing_resolve(
        &persistent.actor,
        options.selector.as_deref(),
        options.revision.as_deref(),
        persistent.handle.session_id(),
        &branch.id,
        &options.role,
    )? else {
        return Ok(None);
    };
    let strategy = &resolved.strategy.definition;
    let profiles = preflight_strategy(environment.file_config, strategy)?;
    let input = routing_input(
        &resolved.strategy.revision_id,
        strategy,
        &options.role,
        &options.lineage,
        prompt,
        environment.request_timeout,
    )?;
    let input_sha = input.digest()?;
    let candidate_sha = input.candidate_ids_digest()?;

    match strategy.mode {
        RoutingMode::Fixed => {
            let target_id = strategy
                .fixed_target
                .as_deref()
                .ok_or(Error::Storage("fixed routing strategy lost its target"))?;
            let target = target(strategy, target_id).map_err(|error| Error::Config(error.to_string()))?;
            let selection = register_target(agent, &persistent.store, &profiles, target, environment.idle_timeout)?;
            let operation = persistent.store.routing_admit_resolved(
                &persistent.actor,
                &resolved.strategy.revision_id,
                persistent.handle.session_id(),
                &branch.id,
                &options.role,
                &options.lineage,
                &input_sha,
                &candidate_sha,
                RoutingSelectionSource::Fixed,
                target_id,
            )?;
            Ok(Some(RouteDecision { selection, operation }))
        }
        RoutingMode::RouteOnce => {
            let affinity = persistent.store.routing_affinity(
                &persistent.actor,
                persistent.handle.session_id(),
                &branch.id,
                &options.role,
                &options.lineage,
            )?;
            if let Some(affinity) = affinity
                .as_ref()
                .filter(|affinity| affinity.strategy_revision_id == resolved.strategy.revision_id)
            {
                let target = target(strategy, &affinity.target_id)
                    .map_err(|error| Error::Config(error.to_string()))?;
                let selection = register_target(agent, &persistent.store, &profiles, target, environment.idle_timeout)?;
                // The fresh selection is the authority check. `grant_revision` is
                // intentionally process-local and is never persisted as affinity
                // authority; destination/model pins below survive process restart.
                selection.revalidate()?;
                let operation = persistent.store.routing_admit_resolved(
                    &persistent.actor,
                    &resolved.strategy.revision_id,
                    persistent.handle.session_id(),
                    &branch.id,
                    &options.role,
                    &options.lineage,
                    &input_sha,
                    &candidate_sha,
                    RoutingSelectionSource::Affinity,
                    &affinity.target_id,
                )?;
                return Ok(Some(RouteDecision { selection, operation }));
            }
            classify(
                agent,
                persistent,
                &profiles,
                resolved.strategy.revision_id.as_str(),
                strategy,
                options,
                input,
                affinity.map(|affinity| affinity.revision),
                environment.idle_timeout,
                cancel,
            )
            .await
            .map(Some)
        }
        RoutingMode::RouteEachPrompt => classify(
            agent,
            persistent,
            &profiles,
            resolved.strategy.revision_id.as_str(),
            strategy,
            options,
            input,
            None,
            environment.idle_timeout,
            cancel,
        )
        .await
        .map(Some),
    }
}

#[allow(clippy::too_many_arguments)]
async fn classify<C>(
    agent: &Agent,
    persistent: &PersistentSession,
    profiles: &BTreeMap<String, config::Profile>,
    revision_id: &str,
    strategy: &aw_routing::RoutingStrategyDefinition,
    options: &RoutingOptions,
    input: RoutingInput,
    affinity_expected_revision: Option<u64>,
    idle_timeout: std::time::Duration,
    cancel: C,
) -> Result<RouteDecision>
where
    C: Future<Output = ()> + Send,
{
    let classifier = strategy
        .classifier
        .as_ref()
        .ok_or(Error::Storage("classified routing strategy lost its classifier"))?;
    let classifier_selection =
        register_target(agent, &persistent.store, profiles, classifier, idle_timeout)?;
    // Construct the tool-free evaluator before durable admission so a local
    // extension/configuration error cannot leave an unstarted route behind.
    let executor = agent.routing_executor()?;
    let branch = persistent.handle.branch()?;
    let operation = persistent.store.routing_admit_classifier(
        &persistent.actor,
        revision_id,
        persistent.handle.session_id(),
        &branch.id,
        &options.role,
        &options.lineage,
        &input.digest()?,
        &input.candidate_ids_digest()?,
    )?;
    let journal = match persistent
        .store
        .routing_classifier_journal(persistent.actor.clone(), &operation.id)
    {
        Ok(journal) => journal,
        Err(error) => {
            persistent.store.routing_settle_unstarted(
                &persistent.actor,
                &operation.id,
                matches!(error, Error::Cancelled),
                &error.to_string(),
            )?;
            return Err(error);
        }
    };
    let result = match executor
        .execute(&classifier_selection, &input, &journal, cancel)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            // This CAS can only settle an operation whose provider intent was
            // never persisted. If dispatch intent exists, it returns false and
            // normal settlement/recovery keeps unknown-effect semantics.
            persistent.store.routing_settle_unstarted(
                &persistent.actor,
                &operation.id,
                matches!(error, Error::Cancelled),
                &error.to_string(),
            )?;
            return Err(error);
        }
    };
    if result.outcome != aw::conversation::Outcome::Completed {
        return Err(Error::Protocol(
            "routing classifier did not produce one valid completed candidate; inspect `aw route status` before retrying",
        ));
    }
    let target_id = result
        .proposal
        .as_ref()
        .ok_or(Error::Protocol("completed routing decision has no candidate"))?
        .candidate_id
        .clone();
    let target = target(strategy, &target_id).map_err(|error| Error::Config(error.to_string()))?;
    let selection = register_target(agent, &persistent.store, profiles, target, idle_timeout)?;
    selection.revalidate()?;

    if strategy.mode == RoutingMode::RouteOnce {
        persistent.store.routing_affinity_establish(
            &persistent.actor,
            &operation.id,
            affinity_expected_revision,
        )?;
    }
    let operation = persistent
        .store
        .routing_operation(&persistent.actor, &operation.id)?;
    Ok(RouteDecision { selection, operation })
}


pub(crate) fn select_routed_retry(
    agent: &Agent,
    persistent: &PersistentSession,
    file_config: &config::FileConfig,
    idle_timeout: std::time::Duration,
    routing_operation_id: &str,
    original: &aw_agent_core::InvocationIdentity,
) -> Result<ProviderSelection> {
    let operation = persistent
        .store
        .routing_operation(&persistent.actor, routing_operation_id)?;
    if operation.consumed_at.is_none() {
        return Err(Error::Conflict(
            "routed retry references a route selection that never admitted working Chat",
        ));
    }
    let recorded = operation
        .working_identity
        .as_ref()
        .ok_or(Error::Storage("consumed routing receipt has no working identity"))?;
    if recorded.actor != original.actor
        || recorded.account_id != original.account_id
        || recorded.account_owner != original.account_owner
        || recorded.payer != original.payer
        || recorded.credential_custody != original.credential_custody
        || recorded.inference_executor != original.inference_executor
        || recorded.model != original.model
    {
        return Err(Error::Conflict(
            "routed retry identity differs from the original logical operation",
        ));
    }
    let strategy = persistent.store.routing_strategy(
        &persistent.actor,
        &operation.strategy_id,
        Some(&operation.revision_id),
    )?;
    let target_id = operation
        .selected_target_id
        .as_deref()
        .ok_or(Error::Storage("routed operation has no selected target"))?;
    let target = target(&strategy.definition, target_id)
        .map_err(|error| Error::Config(error.to_string()))?;
    if target.account_id != original.account_id || target.model != original.model {
        return Err(Error::Conflict(
            "routed retry target differs from the original account/model",
        ));
    }
    let profiles = preflight_strategy(file_config, &strategy.definition)?;
    let selected = register_target(agent, &persistent.store, &profiles, target, idle_timeout)?;
    selected.revalidate()?;
    Ok(selected)
}
