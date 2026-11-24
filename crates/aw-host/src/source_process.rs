//! SQLite-backed host composition of a prepared source effect and the
//! original shared Process runtime. No second Process, Job or policy owner.
use crate::AuthenticatedToolGrantEvidence;
use aw_agent_core::{
    tool::ToolBindingAuthority, CancellationToken, Error, NamedSourceAccess, Result, ToolEffect,
};
use aw_store::{
    PreparedSandboxEffect, SandboxEffectAuthority, SandboxEffectLease, SandboxEffectOutcome,
    SandboxGrantPolicy,
};
use aw_tools::{
    execution_stop::StopEvidence,
    filesystem::runner::{ProcessExit, ProcessTerminal},
};
use aw_tools_adapter::process::{PipeLaunchFailure, PipeProcess, PreparedPipeLaunch};
use tokio::time::Instant;

#[derive(Clone)]
struct LiveProcessTool {
    authority: ToolBindingAuthority,
    evidence: AuthenticatedToolGrantEvidence,
}

impl LiveProcessTool {
    fn is_live(&self) -> bool {
        self.evidence.revalidate(&self.authority).is_ok()
    }

    async fn wait_until_not_live(&self) {
        self.authority
            .wait_until_not_live(&self.evidence.actor, self.evidence.binding())
            .await
    }
}

/// Combine the independent live host source-policy guard with the original
/// ToolBindingAuthority. No tool grant is minted or broadened here. The
/// original host supplies source ceilings and an exact Process tool binding
/// scoped to this Sandbox, rather than a model-selected capability.
pub struct ProcessToolAuthority<A> {
    source: A,
    tool: LiveProcessTool,
}

impl<A: SandboxEffectAuthority> ProcessToolAuthority<A> {
    pub fn new(
        source: A,
        authority: ToolBindingAuthority,
        evidence: AuthenticatedToolGrantEvidence,
    ) -> Result<Self> {
        let policy = source.policy();
        if evidence.actor != policy.actor
            || evidence.resource_scope != policy.sandbox_ref
            || evidence.binding().effect != ToolEffect::Process
        {
            return Err(Error::Denied(
                "Process tool authority does not cover the exact Sandbox actor",
            ));
        }
        evidence.revalidate(&authority)?;
        Ok(Self {
            source,
            tool: LiveProcessTool {
                authority,
                evidence,
            },
        })
    }
}

impl<A: SandboxEffectAuthority> SandboxEffectAuthority for ProcessToolAuthority<A> {
    fn policy(&self) -> &SandboxGrantPolicy {
        self.source.policy()
    }

    fn tool_ceiling(&self) -> &[NamedSourceAccess] {
        self.source.tool_ceiling()
    }

    fn tool_binding(&self) -> Option<&aw_agent_core::tool::ToolBindingSnapshot> {
        Some(self.tool.evidence.binding())
    }

    fn with_admission<T>(&self, admit: impl FnOnce() -> Result<T>) -> Result<T> {
        // Both preflight and the final Store commit use the existing tool
        // publication fence. Never hold its synchronous lock across await.
        self.source.with_admission(|| {
            self.tool.authority.with_revalidated_binding(
                &self.tool.evidence.actor,
                self.tool.evidence.binding(),
                admit,
            )
        })
    }
}

/// The original Process or its factual launch failure, with common source
/// custody. Keep the actual result by value; no allocation or second
/// Process lifecycle. Record the result before settling its source lease.
#[must_use = "retain source custody until the original effect is recorded and settled"]
pub struct SourceProcessStart<'a> {
    pub launch: std::result::Result<PipeProcess, PipeLaunchFailure>,
    pub source: SourceProcessLease<'a>,
}

/// Only source custody and the original ToolBinding revocation watch.
/// Waiting, stopping, stdin, output and terminal facts remain on PipeProcess.
#[must_use = "settle using the original recorded Process terminal and source quiescence"]
pub struct SourceProcessLease<'a> {
    source: SandboxEffectLease<'a>,
    tool: LiveProcessTool,
}

impl SourceProcessLease<'_> {
    /// Compose with the original Process owner's normal cancellation future.
    pub async fn wait_until_not_live(&self) {
        self.tool.wait_until_not_live().await
    }

    /// Call after the original effect owner records the actual launch
    /// failure. Definite no-spawn is neutral; uncertainty stays fenced.
    pub fn settle_after_recorded_failure(
        self,
        failure: &PipeLaunchFailure,
    ) -> Result<SandboxEffectOutcome> {
        let outcome = match failure {
            PipeLaunchFailure::NotStarted(_) => SandboxEffectOutcome::NotStarted,
            PipeLaunchFailure::Unknown(_) => SandboxEffectOutcome::Unknown,
        };
        self.source.settle(outcome)?;
        Ok(outcome)
    }

    /// The original effect owner supplies its exact recorded ProcessTerminal
    /// and the reviewed driver's proof that no work can still access the
    /// sources. A signal reaching a target, or a direct Unix group scan,
    /// is not by itself proof of source quiescence. Output completeness is
    /// separate from physical mutation settlement.
    pub fn settle_after_recorded_terminal(
        self,
        terminal: &ProcessTerminal,
        prove_quiescent: impl FnOnce(&ProcessTerminal) -> Result<()>,
    ) -> Result<SandboxEffectOutcome> {
        if terminal.exit == ProcessExit::Unknown
            || !terminal
                .stop
                .is_some_and(|stop| stop.evidence != StopEvidence::Unknown)
        {
            self.source.settle(SandboxEffectOutcome::Unknown)?;
            return Ok(SandboxEffectOutcome::Unknown);
        }
        match prove_quiescent(terminal) {
            Ok(()) => {
                self.source.settle(SandboxEffectOutcome::Settled)?;
                Ok(SandboxEffectOutcome::Settled)
            }
            Err(error) => {
                self.source.settle(SandboxEffectOutcome::Unknown)?;
                Err(error)
            }
        }
    }
}

/// Consume a registered driver's prepared launch through the original
/// PipeProcess owner. The caller already owns durable effect intent and
/// must record the actual launch/terminal facts before releasing source
/// custody. The driver is responsible for real containment, not merely
/// choosing a working directory. This does not open held Agent admission.
pub async fn start_source_process<'a, P, A>(
    prepared: PreparedSandboxEffect<'a, PreparedPipeLaunch<P>, ProcessToolAuthority<A>>,
    deadline: Instant,
    cancellation: CancellationToken,
) -> Result<SourceProcessStart<'a>>
where
    P: Send + 'static,
    A: SandboxEffectAuthority + Send + 'static,
{
    let tool = prepared.authority().tool.clone();
    let check_tool = tool.clone();
    let signal_tool = tool.clone();
    let check_cancel = cancellation.clone();
    let (launch, source) = prepared
        .start_with_authority(move |launch, authority| {
            PipeProcess::spawn_prepared_until(
                launch.with_custody(authority),
                deadline,
                move || check_cancel.is_cancelled() || !check_tool.is_live(),
                async move {
                    tokio::select! {
                        _ = cancellation.cancelled() => {},
                        _ = signal_tool.wait_until_not_live() => {},
                    }
                },
            )
        })
        .await?;
    Ok(SourceProcessStart {
        launch,
        source: SourceProcessLease { source, tool },
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use aw_agent_core::{
        provider::{CompletionFuture, DeltaSink, Provider},
        AccountOwner, Agent, AgentOptions, BillingOwner, CheckpointPolicy, CredentialCustody,
        DriverKey, InvocationIdentity, JournalEntry, Message, Outcome, PrincipalId, Role,
        SandboxDriverRegistry, SchemaSupport, SourceAccess, SourceBinding, SourceDriverRegistry,
        SourceDriverSchemas, SourceName, SourceSetLimit, ToolCall, ToolOutput, ToolRecord,
        ToolSource, ToolSourceEvidence, ToolStatus, Turn, TurnJournal, TurnStart,
    };
    use aw_store::{
        SandboxEffectOrigin, SandboxEffectPlan, SandboxEffectRequest, SandboxPolicyEvidence,
        SandboxSourceGrant, SessionBinding, SessionStore, SourceMutationOutcome, SourceMutationSet,
        VersionedSourceBinding,
    };
    use aw_tools_adapter::{LocalToolOptions, ToolMode};
    use rusqlite::{params, Connection};
    use std::{path::PathBuf, process::Command, sync::Arc, time::Duration};
    use tokio::sync::{OwnedRwLockReadGuard, RwLock};

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct NoInference;
    impl Provider for NoInference {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Denied("fixture cannot invoke a provider")) })
        }
    }

    struct HostGuard {
        _guard: OwnedRwLockReadGuard<()>,
        policy: SandboxGrantPolicy,
        tools: Vec<NamedSourceAccess>,
    }

    impl SandboxEffectAuthority for HostGuard {
        fn policy(&self) -> &SandboxGrantPolicy {
            &self.policy
        }
        fn tool_ceiling(&self) -> &[NamedSourceAccess] {
            &self.tools
        }
    }

    // Fixed trusted fixture commands, not a general shell containment
    // implementation. Real Process/launch custody is exercised below.
    struct FixtureDriver {
        bash: PathBuf,
    }

    struct LaunchPin(Arc<RwLock<()>>);
    impl Drop for LaunchPin {
        fn drop(&mut self) {
            assert!(
                self.0.try_write().is_err(),
                "host guard must outlive launch pins"
            );
        }
    }

    impl FixtureDriver {
        fn prepare(
            &self,
            plan: &SandboxEffectPlan,
            missing: bool,
            lock: Arc<RwLock<()>>,
        ) -> PreparedPipeLaunch<LaunchPin> {
            assert_eq!(plan.grant.sources.len(), 1);
            assert_eq!(plan.grant.sources[0].access, SourceAccess::ReadWrite);
            let program = if missing {
                plan.sandbox.working_directory.join("missing-program")
            } else {
                self.bash.clone()
            };
            let mut command = Command::new(program);
            command
                .env_clear()
                .current_dir(&plan.sandbox.working_directory)
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    "printf changed > code; printf done",
                ]);
            PreparedPipeLaunch::from_trusted_command(command, false, LaunchPin(lock))
        }
    }

    fn bash() -> PathBuf {
        std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_BASH").map(PathBuf::from))
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            })
            .expect("real Process regression requires host-approved bash")
    }

    fn record(id: &str, status: ToolStatus, output: Option<ToolOutput>) -> ToolRecord {
        ToolRecord {
            call: ToolCall {
                id: id.into(),
                name: "bash".into(),
                arguments: r#"{"command":"printf changed > code; printf done"}"#
                    .parse()
                    .unwrap(),
            },
            effect: ToolEffect::Process,
            execution_site: "local".into(),
            status,
            output,
        }
    }

    async fn finish_tool(
        journal: &impl TurnJournal,
        id: &str,
        status: ToolStatus,
        output: ToolOutput,
    ) -> ToolRecord {
        let record = record(id, status, Some(output));
        journal
            .append(
                1,
                JournalEntry::ToolFinished {
                    record: record.clone(),
                },
            )
            .await
            .unwrap();
        record
    }

    #[tokio::test]
    async fn source_process_handoff_uses_original_spawn_and_tool_authority() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = Temp(
            std::env::temp_dir().join(format!("aw-source-process-{}-{nonce}", std::process::id())),
        );
        std::fs::create_dir(&temp.0).unwrap();
        let source_root = temp.0.join("sources");
        std::fs::create_dir(&source_root).unwrap();
        let code = source_root.join("code");
        std::fs::write(&code, b"initial").unwrap();

        let actor = PrincipalId::new("internal:fixture:source-process").unwrap();
        let drivers = SourceDriverRegistry::new();
        let driver_key = DriverKey::builtin("fixture-source").unwrap();
        let schema = SchemaSupport::new(1, 1).unwrap();
        drivers
            .register(
                driver_key.clone(),
                SourceDriverSchemas {
                    binding: schema,
                    anchor: schema,
                    recovery: schema,
                },
                (),
            )
            .unwrap();
        let limit = SourceSetLimit::new(1).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, drivers.clone(), limit).unwrap();
        let layout = SessionBinding {
            working_directory: source_root.clone(),
            tool_root: source_root.clone(),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (session, branch) = store
            .create(&actor, "source Process", layout.clone())
            .unwrap();
        let (reference, project, user, domain): (String, String, String, String) = {
            let connection = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            connection.query_row(
                "SELECT '/d:'||domain_handle||'/i:'||identity_handle||'/p:'||project_handle||'/a:'||agent_handle,
                        '/d:'||domain_handle||'/i:'||identity_handle||'/p:'||project_handle,
                        owner_handle,domain_handle FROM agent_state_current
                 WHERE session_id=?1 AND branch_id=?2",
                params![session.id, branch.id],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).unwrap()
        };
        let binding = SourceBinding {
            name: SourceName::new("code").unwrap(),
            driver: driver_key,
            binding_schema: 1,
            source_ref: "fixture:code".into(),
            materialization: code.to_str().unwrap().into(),
            access: SourceAccess::ReadWrite,
            checkpoint_policy: CheckpointPolicy::Required,
        };
        let sandbox = store
            .sandbox_create_fresh_with_sources(
                &actor,
                &project,
                &layout,
                std::slice::from_ref(&binding),
            )
            .unwrap();
        // Exercise the original journal owner with an admitted source-free
        // Turn, then install the held source association only in this fixture.
        // No production path bypasses held Agent admission.
        let identity = InvocationIdentity {
            actor: actor.clone(),
            account_id: "fixture".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("fixture".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "fixture".into(),
            grant_revision: 1,
        };
        let (_, journal) = store.activate(&actor, &session.id, &branch.id).unwrap();
        journal
            .begin(TurnStart {
                turn_id: 1,
                prompt: "source effect fixture".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        let calls = [
            "host:stale",
            "host:cancelled",
            "host:missing",
            "host:write",
            "host:unproven",
        ];
        let mut assistant = Message::text(1025, Role::Assistant, "");
        assistant.tool_calls = calls
            .iter()
            .map(|id| record(id, ToolStatus::Running, None).call)
            .collect();
        journal
            .append(1, JournalEntry::Assistant { message: assistant })
            .await
            .unwrap();
        for id in calls {
            journal
                .append(
                    1,
                    JournalEntry::ToolStarted {
                        record: record(id, ToolStatus::Running, None),
                        source: Some(ToolSourceEvidence::trusted_host()),
                    },
                )
                .await
                .unwrap();
        }
        let mut final_records = Vec::new();
        {
            let connection = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            connection.execute(
                "UPDATE agent_state_current SET sandbox_id=?1,admission='held' WHERE session_id=?2 AND branch_id=?3",
                params![sandbox.canonical_ref, session.id, branch.id],
            ).unwrap();
        }
        let policy = SandboxGrantPolicy {
            agent_ref: reference.clone(),
            sandbox_ref: sandbox.canonical_ref.clone(),
            actor: actor.clone(),
            actor_user_id: user,
            evidence: SandboxPolicyEvidence {
                policy_ref: "host:fixture-policy".into(),
                policy_digest: [1; 32],
                authority_ref: "host:fixture-authority".into(),
                authority_generation: 1,
            },
            sources: vec![SandboxSourceGrant::from_binding(
                &VersionedSourceBinding {
                    binding: binding.clone(),
                    binding_rev: 1,
                },
                SourceAccess::ReadWrite,
            )],
        };
        store
            .sandbox_grant_set(&actor, &reference, 0, Some(&policy))
            .unwrap();
        let tools = vec![NamedSourceAccess {
            name: binding.name.clone(),
            access: SourceAccess::ReadWrite,
        }];
        let set = SourceMutationSet::from_bindings(&[binding]).unwrap();

        let mut options = LocalToolOptions::new(&source_root, ToolMode::Trusted);
        options.bash = Some(bash());
        options.selected_tools = vec!["bash".into()];
        let builder = Agent::builder(
            Arc::new(NoInference),
            AgentOptions::new("fixture", "fixture"),
        );
        let mut agent = crate::configure_local(builder, actor.clone(), &options)
            .unwrap()
            .build()
            .unwrap();
        let tool_authority = agent.tool_binding_authority();
        let capture = || {
            AuthenticatedToolGrantEvidence::capture(
                &tool_authority,
                &actor,
                "bash",
                "tools.bash",
                format!("d:{domain}"),
                &sandbox.canonical_ref,
                "host:source-process",
            )
            .unwrap()
        };

        let sandbox_drivers = SandboxDriverRegistry::new();
        let handle = sandbox_drivers
            .register(
                DriverKey::builtin("fixture-process").unwrap(),
                FixtureDriver { bash: bash() },
            )
            .unwrap();
        let reader = sandbox_drivers.reader().unwrap();
        let policy_lock = Arc::new(RwLock::new(()));
        let prepare = |effect_id: &str, evidence: AuthenticatedToolGrantEvidence, missing: bool| {
            store.sandbox_prepare_source_effect(
                SandboxEffectRequest {
                    actor: actor.clone(),
                    agent_ref: reference.clone(),
                    origin: SandboxEffectOrigin::Tool {
                        turn_id: 1,
                        call_id: effect_id.into(),
                    },
                    sources: tools.clone(),
                },
                &reader,
                &handle,
                |_| {
                    ProcessToolAuthority::new(
                        HostGuard {
                            _guard: policy_lock.clone().try_read_owned().unwrap(),
                            policy: policy.clone(),
                            tools: tools.clone(),
                        },
                        tool_authority.clone(),
                        evidence,
                    )
                },
                |driver, plan, _| Ok(driver.prepare(plan, missing, policy_lock.clone())),
            )
        };
        let deadline = || Instant::now() + Duration::from_secs(5);
        assert!(prepare("host:absent", capture(), false).is_err());

        let stale = prepare("host:stale", capture(), false).unwrap();
        agent
            .revoke_registered_source_tool(&actor, "bash", &ToolSource::trusted_host())
            .unwrap();
        assert!(
            start_source_process(stale, deadline(), CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&code).unwrap(), b"initial");
        assert!(policy_lock.try_write().is_ok());
        final_records.push(
            finish_tool(
                &journal,
                "host:stale",
                ToolStatus::Failed,
                ToolOutput::error("revoked before launch"),
            )
            .await,
        );
        store
            .acquire_source_mutation_barrier(&set, "host:after-stale")
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();
        agent
            .permit_registered_host_tool(actor.clone(), "bash")
            .unwrap();

        let cancel = CancellationToken::new();
        cancel.cancel();
        for (id, missing, cancellation) in [
            ("host:cancelled", false, cancel),
            ("host:missing", true, CancellationToken::new()),
        ] {
            let prepared = prepare(id, capture(), missing).unwrap();
            let SourceProcessStart { launch, source } =
                start_source_process(prepared, deadline(), cancellation)
                    .await
                    .unwrap();
            let Err(failure) = launch else {
                panic!("definite no-spawn returned a Process")
            };
            assert!(matches!(failure, PipeLaunchFailure::NotStarted(_)));
            final_records.push(
                finish_tool(
                    &journal,
                    id,
                    ToolStatus::Failed,
                    ToolOutput::error("definite no-spawn"),
                )
                .await,
            );
            assert_eq!(
                source.settle_after_recorded_failure(&failure).unwrap(),
                SandboxEffectOutcome::NotStarted
            );
        }
        assert_eq!(
            store
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev
        );

        let prepared = prepare("host:write", capture(), false).unwrap();
        let effect_id = prepared.plan().effect_id.clone();
        assert!(store
            .acquire_source_mutation_barrier(&set, &effect_id)
            .is_err());
        let duplicate = prepare("host:write", capture(), false).unwrap();
        let SourceProcessStart { launch, source } =
            start_source_process(prepared, deadline(), CancellationToken::new())
                .await
                .unwrap();
        let process = launch.unwrap();
        assert!(
            start_source_process(duplicate, deadline(), CancellationToken::new())
                .await
                .is_err()
        );
        assert!(policy_lock.try_write().is_ok());
        assert!(store
            .acquire_source_mutation_barrier(&set, "host:running")
            .is_err());
        let receipt = process
            .wait(source.wait_until_not_live(), Duration::from_secs(5))
            .await;
        assert_eq!(receipt.terminal.exit, ProcessExit::Code(0));
        assert_eq!(receipt.output.stdout.head, b"done");
        assert_eq!(std::fs::read(&code).unwrap(), b"changed");
        assert!(store
            .acquire_source_mutation_barrier(&set, "host:unsettled")
            .is_err());
        agent
            .revoke_registered_source_tool(&actor, "bash", &ToolSource::trusted_host())
            .unwrap();
        source.wait_until_not_live().await;
        final_records.push(
            finish_tool(
                &journal,
                "host:write",
                ToolStatus::Succeeded,
                ToolOutput::text("done"),
            )
            .await,
        );
        let agent_before = store.agent_get(&actor, &reference).unwrap();
        // The fixture's fixed command contains only shell builtins and
        // cannot create descendants. This is not a containment proof for
        // arbitrary shell commands.
        assert_eq!(
            source
                .settle_after_recorded_terminal(&receipt.terminal, |_| Ok(()))
                .unwrap(),
            SandboxEffectOutcome::Settled
        );
        assert_eq!(
            store
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev + 1
        );
        assert_eq!(store.agent_get(&actor, &reference).unwrap(), agent_before);
        assert_eq!(
            store
                .sandbox_recover_tool_source_effect(
                    &actor,
                    &reference,
                    1,
                    "host:write",
                    |_| panic!("released custody must not inspect or execute")
                )
                .unwrap(),
            SandboxEffectOutcome::Settled
        );
        assert!(store.activate(&actor, &session.id, &branch.id).is_err());

        agent
            .permit_registered_host_tool(actor.clone(), "bash")
            .unwrap();
        let prepared = prepare("host:unproven", capture(), false).unwrap();
        let unresolved_id = prepared.plan().effect_id.clone();
        let SourceProcessStart { launch, source } =
            start_source_process(prepared, deadline(), CancellationToken::new())
                .await
                .unwrap();
        let receipt = launch
            .unwrap()
            .wait(std::future::pending(), Duration::from_secs(5))
            .await;
        assert_eq!(receipt.terminal.exit, ProcessExit::Code(0));
        // Even a known physical terminal cannot release source custody
        // before the matching immutable original tool receipt exists.
        assert!(source
            .settle_after_recorded_terminal(&receipt.terminal, |_| Ok(()))
            .is_err());
        assert!(store
            .acquire_source_mutation_barrier(&set, "host:unknown")
            .is_err());
        assert!(store
            .sandbox_recover_tool_source_effect(&actor, &reference, 1, "host:unproven", |_| panic!(
                "missing original terminal cannot release or inspect"
            ))
            .is_err());
        assert!(prepare("host:unproven", capture(), false).is_err());
        final_records.push(
            finish_tool(
                &journal,
                "host:unproven",
                ToolStatus::Succeeded,
                ToolOutput::text("done"),
            )
            .await,
        );

        // A durable logical receipt alone still cannot prove quiescence.
        assert!(store
            .sandbox_recover_tool_source_effect(
                &actor,
                &reference,
                1,
                "host:unproven",
                |recovery| {
                    assert_eq!(recovery.effect_id, unresolved_id);
                    assert_eq!(recovery.original.call, recovery.terminal.call);
                    Err(Error::Conflict(
                        "fixture cannot prove source quiescence yet",
                    ))
                }
            )
            .is_err());
        journal
            .append(
                1,
                JournalEntry::Assistant {
                    message: Message::text(1026, Role::Assistant, "fixture complete"),
                },
            )
            .await
            .unwrap();
        journal
            .finish(Turn {
                id: 1,
                prompt: "source effect fixture".into(),
                answer: "fixture complete".into(),
                model: identity.model,
                profile: identity.account_id,
                outcome: Outcome::Completed,
                meta: None,
                error: None,
                actor: Some(actor.clone()),
                tool_records: final_records,
                invocations: Vec::new(),
                context_messages: Vec::new(),
                needs_reconciliation: false,
            })
            .await
            .unwrap();
        drop(journal);
        drop(store);

        let reopened = SessionStore::open(&temp.0).unwrap();
        assert!(reopened
            .acquire_source_mutation_barrier(&set, "host:restart")
            .is_err());
        assert!(reopened
            .sandbox_recover_tool_source_effect(&actor, &reference, 1, "host:unproven", |_| panic!(
                "missing SourceDriver cannot inspect"
            ))
            .is_err());
        drop(reopened);
        let reopened = SessionStore::open_with_sources(&temp.0, drivers, limit).unwrap();
        assert_eq!(
            reopened
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev + 1
        );
        assert_eq!(
            reopened
                .sandbox_recover_tool_source_effect(
                    &actor,
                    &reference,
                    1,
                    "host:unproven",
                    |recovery| {
                        assert_eq!(recovery.effect_id, unresolved_id);
                        assert_eq!(recovery.actor, actor);
                        assert_eq!(&recovery.driver, &handle.descriptor().driver);
                        assert_eq!(recovery.original.call.id, "host:unproven");
                        assert_eq!(recovery.terminal.status, ToolStatus::Succeeded);
                        assert_eq!(recovery.sources.len(), 1);
                        assert_eq!(
                            recovery.sources[0].binding.binding.materialization,
                            code.to_str().unwrap()
                        );
                        assert!(reopened
                            .acquire_source_mutation_barrier(&set, "host:recovering")
                            .is_err());
                        Ok(SandboxEffectOutcome::Settled)
                    }
                )
                .unwrap(),
            SandboxEffectOutcome::Settled
        );
        assert_eq!(
            reopened
                .sandbox_recover_tool_source_effect(
                    &actor,
                    &reference,
                    1,
                    "host:unproven",
                    |_| panic!("released custody cannot replay inspection")
                )
                .unwrap(),
            SandboxEffectOutcome::Settled
        );
        assert_eq!(
            reopened
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev + 2
        );
        reopened
            .acquire_source_mutation_barrier(&set, "host:after-recovery")
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();
        assert!(reopened.activate(&actor, &session.id, &branch.id).is_err());
    }
}
