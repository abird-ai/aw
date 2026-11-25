//! SQLite-backed host composition of a prepared source effect and the
//! original shared Process runtime. No second Process, Job or policy owner.
use crate::source_authority::{LiveSourceTool, SourceToolAuthority};
use aw_agent_core::{CancellationToken, Error, Result};
use aw_store::{
    PreparedSandboxEffect, SandboxEffectAuthority, SandboxEffectLaunch, SandboxEffectLease,
    SandboxEffectOutcome,
};
use aw_tools::{
    execution_stop::StopEvidence,
    filesystem::runner::{ProcessExit, ProcessTerminal},
};
use aw_tools_adapter::process::{
    PipeLaunchFailure, PipeProcess, PreparedPipeLaunch, ProcessReceipt,
};
use tokio::time::Instant;

/// Existing Process consumers use the same source/tool authority composition.
pub type ProcessToolAuthority<A> = SourceToolAuthority<A>;

/// The original Process or its factual launch failure, with common source
/// custody. Keep the actual result by value; no allocation or second
/// Process lifecycle. Record the result before settling its source lease.
#[must_use = "retain source custody until the original effect is recorded and settled"]
pub struct SourceProcessStart {
    pub launch: std::result::Result<PipeProcess, PipeLaunchFailure>,
    pub source: SourceProcessLease,
}

/// Only source custody and the original ToolBinding revocation watch.
/// Waiting, stopping, stdin, output and terminal facts remain on PipeProcess.
#[must_use = "settle using the original recorded Process terminal and source quiescence"]
pub struct SourceProcessLease {
    source: SandboxEffectLease,
    tool: LiveSourceTool,
}

impl SourceProcessLease {
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
        let outcome = launch_outcome(failure);
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
        let outcome = terminal_outcome(terminal, prove_quiescent);
        self.source.settle(
            outcome
                .as_ref()
                .copied()
                .unwrap_or(SandboxEffectOutcome::Unknown),
        )?;
        outcome
    }
}

fn launch_outcome(failure: &PipeLaunchFailure) -> SandboxEffectOutcome {
    match failure {
        PipeLaunchFailure::NotStarted(_) => SandboxEffectOutcome::NotStarted,
        PipeLaunchFailure::Unknown(_) => SandboxEffectOutcome::Unknown,
    }
}

fn terminal_outcome(
    terminal: &ProcessTerminal,
    prove_quiescent: impl FnOnce(&ProcessTerminal) -> Result<()>,
) -> Result<SandboxEffectOutcome> {
    if terminal.exit == ProcessExit::Unknown
        || !terminal
            .stop
            .is_some_and(|stop| stop.evidence != StopEvidence::Unknown)
    {
        return Ok(SandboxEffectOutcome::Unknown);
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prove_quiescent(terminal)))
        .map_err(|_| Error::Protocol("source quiescence proof panicked; inspect before retry"))??;
    Ok(SandboxEffectOutcome::Settled)
}

#[derive(Clone)]
struct SourceProcessControl {
    tool: LiveSourceTool,
    cancellation: CancellationToken,
    abandoned: Option<CancellationToken>,
}

impl SourceProcessControl {
    fn probe(&self) -> impl Fn() -> bool + Send + 'static {
        let cancellation = self.cancellation.clone();
        let abandoned = self.abandoned.clone();
        move || {
            cancellation.is_cancelled()
                || abandoned
                    .as_ref()
                    .is_some_and(CancellationToken::is_cancelled)
        }
    }

    async fn cancelled(&self) {
        tokio::select! {
            _ = self.cancellation.cancelled() => {},
            _ = self.tool.wait_until_not_live() => {},
            _ = async {
                match &self.abandoned {
                    Some(token) => token.cancelled().await,
                    None => std::future::pending().await,
                }
            } => {},
        }
    }
}

async fn spawn_source_process<P, A>(
    launch: PreparedPipeLaunch<P>,
    authority: ProcessToolAuthority<A>,
    boundary: SandboxEffectLaunch,
    deadline: Instant,
    control: &SourceProcessControl,
) -> std::result::Result<PipeProcess, PipeLaunchFailure>
where
    P: Send + 'static,
    A: SandboxEffectAuthority + Send + 'static,
{
    let check = control.probe();
    let signal = control.clone();
    PipeProcess::spawn_prepared_fenced_until(
        launch.with_custody(authority),
        deadline,
        // ToolBinding is checked under its actual synchronous launch fence.
        // Do not reenter its read lock from the final cancellation probe.
        check,
        async move { signal.cancelled().await },
        move |spawn, (_, authority)| {
            let _ = boundary.with_source_authority(authority, || {
                spawn.spawn();
                Ok(())
            });
        },
    )
    .await
}

/// Factual original Process result and independent, deferred source
/// settlement. A failed proof write must not erase the OS result.
#[must_use = "record the original Process result and check source quiescence"]
pub struct SourceProcessCompletion {
    pub receipt: std::result::Result<ProcessReceipt, PipeLaunchFailure>,
    pub quiescence: Result<SandboxEffectOutcome>,
}

struct CancelAwaiter(CancellationToken);
impl Drop for CancelAwaiter {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Trusted finite, noninteractive Process execution. The actual Process
/// owner holds source custody through wait/stop/reap and records physical
/// quiescence before returning a result to the original journal owner.
/// Losing the awaiter cancels only this execution, not its parent Turn,
/// and does not abort its settlement task. The reviewed driver must prove
/// permanent source quiescence; a negative Unix group scan is insufficient.
/// No new Job, replay or source-bearing admission is introduced.
pub async fn run_source_process<P, A>(
    prepared: PreparedSandboxEffect<'_, PreparedPipeLaunch<P>, ProcessToolAuthority<A>>,
    deadline: Instant,
    cancellation: CancellationToken,
    prove_quiescent: impl FnOnce(&ProcessTerminal) -> Result<()> + Send + 'static,
) -> Result<SourceProcessCompletion>
where
    P: Send + 'static,
    A: SandboxEffectAuthority + Send + 'static,
{
    let abandoned = CancellationToken::new();
    let control = SourceProcessControl {
        tool: prepared.authority().tool.clone(),
        cancellation,
        abandoned: Some(abandoned.clone()),
    };
    let _awaiter = CancelAwaiter(abandoned);
    let (launch, authority, boundary, source) = prepared.admit_with_launch_authority()?;
    tokio::spawn(async move {
        let receipt =
            match spawn_source_process(launch, authority, boundary, deadline, &control).await {
                Ok(mut process) => {
                    process.close_stdin();
                    Ok(process
                        .wait(
                            control.cancelled(),
                            deadline.saturating_duration_since(Instant::now()),
                        )
                        .await)
                }
                Err(failure) => Err(failure),
            };
        let terminal = receipt
            .as_ref()
            .map(|receipt| receipt.terminal)
            .map_err(launch_outcome);
        let quiescence = tokio::task::spawn_blocking(move || {
            let outcome = match terminal {
                Ok(terminal) => terminal_outcome(&terminal, prove_quiescent),
                Err(outcome) => Ok(outcome),
            };
            source.defer_until_tool_finished(
                outcome
                    .as_ref()
                    .copied()
                    .unwrap_or(SandboxEffectOutcome::Unknown),
            )?;
            outcome
        })
        .await
        .unwrap_or(Err(Error::Protocol(
            "source Process settlement worker failed; inspect before retry",
        )));
        SourceProcessCompletion {
            receipt,
            quiescence,
        }
    })
    .await
    .map_err(|_| Error::Protocol("source Process owner failed; inspect before retry"))
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
) -> Result<SourceProcessStart>
where
    P: Send + 'static,
    A: SandboxEffectAuthority + Send + 'static,
{
    let tool = prepared.authority().tool.clone();
    let control = SourceProcessControl {
        tool: tool.clone(),
        cancellation,
        abandoned: None,
    };
    let (launch, authority, boundary, source) = prepared.admit_with_launch_authority()?;
    let launch = spawn_source_process(launch, authority, boundary, deadline, &control).await;
    Ok(SourceProcessStart {
        launch,
        source: SourceProcessLease { source, tool },
    })
}

/// Opt-in provider-defined proof projection through the same original
/// source Process and factual settlement. No generic secret environment,
/// arbitrary target mapping or alternative source/tool admission.
pub async fn start_source_process_with_vault<'a, P, A, S>(
    prepared: PreparedSandboxEffect<'a, PreparedPipeLaunch<P>, ProcessToolAuthority<A>>,
    deadline: Instant,
    cancellation: CancellationToken,
    registry: aw_agent_core::VaultProviderRegistryReader,
    handle: aw_agent_core::VaultProviderHandle<aw_store::SigningVaultProvider<S>>,
    selection: aw_store::SigningProcessProof,
) -> Result<SourceProcessStart>
where
    P: Send + 'static,
    A: SandboxEffectAuthority + Send + 'static,
    S: aw_store::SecretStore + 'static,
{
    let tool = prepared.authority().tool.clone();
    let control = SourceProcessControl {
        tool: tool.clone(),
        cancellation,
        abandoned: None,
    };
    let check = control.probe();
    let (launch, source) = prepared
        .start_with_launch_authority(move |launch, authority, boundary| {
            PipeProcess::spawn_prepared_fenced_until(
                launch.with_custody(authority),
                deadline,
                // The final synchronous fence itself owns the ToolBinding read
                // guard. Do not re-enter that guard from the cancellation probe.
                check,
                async move { control.cancelled().await },
                move |mut spawn, (_, authority)| {
                    // Only the actual registered provider and current original
                    // intent may project this value. A refusal leaves the
                    // one-shot spawn token unused, yielding definite no-spawn.
                    let _ = aw_store::SigningVaultProvider::with_process_proof(
                        &registry,
                        &handle,
                        boundary,
                        authority,
                        selection,
                        |proof| {
                            let mut hex = [0u8; 64];
                            const DIGITS: &[u8; 16] = b"0123456789abcdef";
                            for (byte, pair) in proof.iter().zip(hex.chunks_exact_mut(2)) {
                                pair[0] = DIGITS[(byte >> 4) as usize];
                                pair[1] = DIGITS[(byte & 15) as usize];
                            }
                            spawn
                                .insert_reserved_env(
                                    aw_store::SIGNING_PROOF_ENV,
                                    std::str::from_utf8(&hex).expect("ASCII proof"),
                                )
                                .map_err(|_| {
                                    Error::Denied("reserved Vault projection collision")
                                })?;
                            spawn.spawn();
                            Ok(())
                        },
                    );
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
    use crate::AuthenticatedToolGrantEvidence;
    use aw_agent_core::{
        provider::{CompletionFuture, DeltaSink, Provider},
        AccountOwner, Agent, AgentOptions, BillingOwner, CheckpointPolicy, CredentialCustody,
        DriverKey, InvocationIdentity, JournalEntry, Message, NamedSourceAccess, PrincipalId, Role,
        SandboxDriverRegistry, SchemaSupport, SourceAccess, SourceBinding, SourceDriverRegistry,
        SourceDriverSchemas, SourceName, SourceSetLimit, ToolCall, ToolEffect, ToolOutput,
        ToolRecord, ToolSource, ToolSourceEvidence, ToolStatus, Turn, TurnJournal, TurnStart,
    };
    use aw_store::{
        SandboxEffectOrigin, SandboxEffectPlan, SandboxEffectRequest, SandboxGrantPolicy,
        SandboxPolicyEvidence, SandboxSourceGrant, SessionBinding, SessionStore,
        SourceMutationOutcome, SourceMutationSet, VersionedSourceBinding,
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

    fn fixture_command(id: &str) -> &'static str {
        if id.starts_with("host:vault-") {
            r#"[[ "$AW_VAULT_SIGNING_PROOF" =~ ^[0-9a-f]{64}$ ]] || exit 21; printf changed > code; printf done"#
        } else if id == "host:interrupted" {
            // Fixed single-child workload: shell builtins only. The marker
            // proves actual execution before the result awaiter disappears.
            "printf entered > entered; while :; do :; done"
        } else {
            "printf changed > code; printf done"
        }
    }

    impl FixtureDriver {
        fn prepare(
            &self,
            plan: &SandboxEffectPlan,
            missing: bool,
            lock: Arc<RwLock<()>>,
            id: &str,
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
                .args(["--noprofile", "--norc", "-c", fixture_command(id)]);
            if id == "host:vault-collision" {
                command.env(aw_store::SIGNING_PROOF_ENV, "untrusted-reserved-value");
            }
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
                arguments: serde_json::json!({"command": fixture_command(id)}),
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
            "host:stale-source",
            "host:cancelled",
            "host:missing",
            "host:vault-stale",
            "host:vault-collision",
            "host:vault-valid",
            "host:write",
            "host:unproven",
            "host:interrupted",
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
                        call: record(effect_id, ToolStatus::Running, None).call,
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
                |driver, plan, _| Ok(driver.prepare(plan, missing, policy_lock.clone(), effect_id)),
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

        // Revoke and regrant *after* original admission, before the actual
        // spawn lane. The old decision must not follow the new generation.
        let stale_source = prepare("host:stale-source", capture(), false).unwrap();
        let control = SourceProcessControl {
            tool: stale_source.authority().tool.clone(),
            cancellation: CancellationToken::new(),
            abandoned: None,
        };
        let (launch, authority, boundary, source) =
            stale_source.admit_with_launch_authority().unwrap();
        let revoked = store
            .sandbox_grant_set(&actor, &reference, 1, None)
            .unwrap();
        store
            .sandbox_grant_set(&actor, &reference, revoked.revision, Some(&policy))
            .unwrap();
        let Err(failure) =
            spawn_source_process(launch, authority, boundary, deadline(), &control).await
        else {
            panic!("stale admitted source grant spawned a Process")
        };
        assert!(matches!(
            failure,
            PipeLaunchFailure::NotStarted(Error::Denied(_))
        ));
        assert_eq!(std::fs::read(&code).unwrap(), b"initial");
        final_records.push(
            finish_tool(
                &journal,
                "host:stale-source",
                ToolStatus::Failed,
                ToolOutput::error("source grant changed before spawn"),
            )
            .await,
        );
        source.settle(SandboxEffectOutcome::NotStarted).unwrap();

        let cancel = CancellationToken::new();
        cancel.cancel();
        for (id, missing, cancellation) in [
            ("host:cancelled", false, cancel),
            ("host:missing", true, CancellationToken::new()),
        ] {
            let prepared = prepare(id, capture(), missing).unwrap();
            let completion = run_source_process(prepared, deadline(), cancellation, |_| {
                panic!("no-spawn must not inspect a terminal")
            })
            .await
            .unwrap();
            assert!(matches!(
                completion.receipt,
                Err(PipeLaunchFailure::NotStarted(_))
            ));
            assert_eq!(
                completion.quiescence.unwrap(),
                SandboxEffectOutcome::NotStarted
            );
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
                store
                    .sandbox_settle_recorded_tool_source_effect(&actor, &reference, 1, id)
                    .unwrap(),
                Some(SandboxEffectOutcome::NotStarted)
            );
        }
        assert_eq!(
            store
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev
        );

        // An actual registered provider, using the original encrypted
        // SecretStore under a separate trusted root. Only its proof enters
        // the fixture's fixed child command; the raw key never leaves custody.
        let vault_root = temp.0.join("vault-provider");
        std::fs::create_dir(&vault_root).unwrap();
        let vault_key = DriverKey::builtin("fixture-signing").unwrap();
        let vault_registry = aw_agent_core::VaultProviderRegistry::new();
        let vault_handle = aw_store::SigningVaultProvider::new(
            vault_key.clone(),
            aw_store::LocalSecretStore::open(&vault_root).unwrap(),
        )
        .unwrap()
        .register(&vault_registry)
        .unwrap();
        let provider = vault_registry.reader().unwrap().get(&vault_handle).unwrap();
        let target = aw_store::VaultGrantTarget {
            agent_ref: reference.clone(),
            actor: actor.clone(),
            name: SourceName::new("signing").unwrap(),
            entry: aw_agent_core::VaultEntryRef {
                provider: vault_key,
                entry: "fixture".into(),
            },
            projection: aw_agent_core::VaultProjection::Env,
            scope: "fixture-process".into(),
        };
        let attach = || {
            store
                .agent_vault_attach_signing(
                    &actor,
                    &reference,
                    &target.name,
                    store.agent_get(&actor, &reference).unwrap().rev,
                    &vault_registry.reader().unwrap(),
                    &vault_handle,
                    &target.entry.entry,
                )
                .unwrap()
        };
        provider
            .provision(&store, &actor, target.clone(), &[41; 32])
            .unwrap();
        let stale_rev = attach();
        let stale = prepare("host:vault-stale", capture(), false).unwrap();
        provider
            .provision(&store, &actor, target.clone(), &[73; 32])
            .unwrap();
        let current_rev = attach();
        let selection = |grant_rev| aw_store::SigningProcessProof {
            name: target.name.clone(),
            entry: target.entry.entry.clone(),
            scope: target.scope.clone(),
            grant_rev,
            challenge: b"fixture challenge".to_vec(),
        };
        let collision = prepare("host:vault-collision", capture(), false).unwrap();
        for (id, prepared, rev) in [
            ("host:vault-stale", stale, stale_rev),
            ("host:vault-collision", collision, current_rev),
        ] {
            let SourceProcessStart { launch, source } = start_source_process_with_vault(
                prepared,
                deadline(),
                CancellationToken::new(),
                vault_registry.reader().unwrap(),
                vault_handle.clone(),
                selection(rev),
            )
            .await
            .unwrap();
            let Err(failure) = launch else {
                panic!("refused Vault projection spawned a Process");
            };
            assert!(matches!(
                failure,
                PipeLaunchFailure::NotStarted(Error::Denied(_))
            ));
            assert_eq!(std::fs::read(&code).unwrap(), b"initial");
            final_records.push(
                finish_tool(
                    &journal,
                    id,
                    ToolStatus::Failed,
                    ToolOutput::error("Vault launch refused"),
                )
                .await,
            );
            assert_eq!(
                source.settle_after_recorded_failure(&failure).unwrap(),
                SandboxEffectOutcome::NotStarted
            );
        }
        let valid = prepare("host:vault-valid", capture(), false).unwrap();
        let SourceProcessStart { launch, source } = start_source_process_with_vault(
            valid,
            deadline(),
            CancellationToken::new(),
            vault_registry.reader().unwrap(),
            vault_handle.clone(),
            selection(current_rev),
        )
        .await
        .unwrap();
        let receipt = launch
            .unwrap()
            .wait(source.wait_until_not_live(), Duration::from_secs(5))
            .await;
        assert_eq!(receipt.terminal.exit, ProcessExit::Code(0));
        assert_eq!(receipt.output.stdout.head, b"done");
        assert_eq!(std::fs::read(&code).unwrap(), b"changed");
        final_records.push(
            finish_tool(
                &journal,
                "host:vault-valid",
                ToolStatus::Succeeded,
                ToolOutput::text("done"),
            )
            .await,
        );
        assert_eq!(
            source
                .settle_after_recorded_terminal(&receipt.terminal, |_| Ok(()))
                .unwrap(),
            SandboxEffectOutcome::Settled
        );
        provider.revoke(&target.entry.entry).unwrap();

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
            sandbox.rev + 2
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
        let completion = run_source_process(prepared, deadline(), CancellationToken::new(), |_| {
            Err(Error::Conflict(
                "fixture cannot prove source quiescence yet",
            ))
        })
        .await
        .unwrap();
        // Physical process truth survives failure to prove source quiescence.
        let receipt = completion.receipt.unwrap();
        assert_eq!(receipt.terminal.exit, ProcessExit::Code(0));
        assert_eq!(receipt.output.stdout.head, b"done");
        assert!(completion.quiescence.is_err());
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
        // Existing ToolFinished recovery still works and retains its
        // original numeric release receipt. This exact fixed child has
        // no descendants; its known terminal proves fixture quiescence.
        assert_eq!(
            store
                .sandbox_recover_tool_source_effect(
                    &actor,
                    &reference,
                    1,
                    "host:unproven",
                    |recovery| {
                        assert_eq!(recovery.effect_id, unresolved_id);
                        assert!(matches!(
                            recovery.terminal_ref,
                            aw_store::SandboxToolTerminalRef::ToolFinished(_)
                        ));
                        assert_eq!(recovery.terminal.status, ToolStatus::Succeeded);
                        Ok(SandboxEffectOutcome::Settled)
                    }
                )
                .unwrap(),
            SandboxEffectOutcome::Settled
        );

        // Loss of the awaiter while the real finite Process is running.
        // Its original owner requests cancellation, waits/reaps, and
        // records physical proof independently of the lost result.
        let prepared = prepare("host:interrupted", capture(), false).unwrap();
        let interrupted_id = prepared.plan().effect_id.clone();
        let parent_cancel = CancellationToken::new();
        let mut pending = Box::pin(run_source_process(
            prepared,
            deadline(),
            parent_cancel.clone(),
            |terminal| {
                // The trusted fixture command contains no child creation.
                assert_eq!(
                    terminal.stop_reason(),
                    Some(aw_tools::execution_stop::StopReason::Cancelled)
                );
                Ok(())
            },
        ));
        let entered = source_root.join("entered");
        tokio::select! {
            _ = &mut pending => panic!("finite Process returned before result loss"),
            ready = tokio::time::timeout(Duration::from_secs(3), async {
                while !entered.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }) => ready.unwrap(),
        }
        drop(pending);
        assert!(!parent_cancel.is_cancelled());
        // Wait for the worker's original durable physical fact, not a
        // guessed delay or new process observation. Its Store owner
        // must stay live until this proof commits.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let done: bool = Connection::open(temp.0.join("sessions.sqlite3"))
                    .unwrap()
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM commit_log
                         WHERE entity_type='source_effect' AND entity_id=?1 AND kind='source.effect.quiescent')",
                        [&interrupted_id],
                        |row| row.get(0),
                    )
                    .unwrap();
                if done {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(store
            .sandbox_recover_tool_source_effect(
                &actor,
                &reference,
                1,
                "host:interrupted",
                |_| panic!("a live unfinished Run is not an interrupted terminal")
            )
            .is_err());
        drop(journal);
        drop(store);

        let reopened = SessionStore::open(&temp.0).unwrap();
        assert!(reopened
            .acquire_source_mutation_barrier(&set, "host:restart")
            .is_err());
        assert!(reopened
            .sandbox_recover_tool_source_effect(
                &actor,
                &reference,
                1,
                "host:interrupted",
                |_| panic!("missing SourceDriver cannot inspect")
            )
            .is_err());
        drop(reopened);
        let reopened = SessionStore::open_with_sources(&temp.0, drivers, limit).unwrap();
        assert_eq!(
            reopened
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev + 3
        );
        // Source claims stay fenced until the original Run closure is
        // paired with the worker's recorded physical fact.
        assert!(reopened
            .acquire_source_mutation_barrier(&set, "host:unknown-after-restart")
            .is_err());
        let history = || -> (String, String, u64, String) {
            let c = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            c.query_row(
                "SELECT r.status,n.payload,(SELECT count(*) FROM run_events WHERE run_id=r.id),
                   (SELECT state FROM hazards WHERE run_id=r.id)
                 FROM runs r JOIN nodes n ON n.id=r.final_node WHERE r.session_id=?1 AND r.turn_id=1",
                [&session.id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))
            ).unwrap()
        };
        let original_history = history();
        assert_eq!(original_history.0, "interrupted");
        assert_eq!(original_history.3, "needs_inspection");
        final_records.push(record("host:interrupted", ToolStatus::Interrupted, None));
        let original_turn: Turn = serde_json::from_str(&original_history.1).unwrap();
        assert_eq!(
            serde_json::to_value(&original_turn.tool_records).unwrap(),
            serde_json::to_value(&final_records).unwrap()
        );
        assert_eq!(
            reopened
                .sandbox_recover_tool_source_effect(
                    &actor,
                    &reference,
                    1,
                    "host:interrupted",
                    |_| panic!("recorded Process quiescence needs no external reinspection")
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
                    "host:interrupted",
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
            sandbox.rev + 4
        );
        // No rewritten history, new tool receipt or blanket Run-hazard
        // acknowledgement from physical source settlement.
        assert_eq!(history(), original_history);
        reopened
            .acquire_source_mutation_barrier(&set, "host:after-recovery")
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();
        assert!(reopened.activate(&actor, &session.id, &branch.id).is_err());
        drop(reopened);
        // Terminal replay is lookup-only even on a later degraded owner
        // reopen. The broader unresolved Run still gates future effects.
        let reopened = SessionStore::open(&temp.0).unwrap();
        for id in ["host:unproven", "host:interrupted"] {
            assert_eq!(
                reopened
                    .sandbox_recover_tool_source_effect(&actor, &reference, 1, id, |_| panic!(
                        "terminal replay requires no provider or inspection"
                    ))
                    .unwrap(),
                SandboxEffectOutcome::Settled
            );
        }
        assert_eq!(history(), original_history);
        assert_eq!(
            reopened
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev + 4
        );
        assert!(reopened.activate(&actor, &session.id, &branch.id).is_err());
    }

    // Original route receives an existing Tool. The source-aware route must
    // never invoke an ambient-root fallback, including on a denied path.
    struct NoFallback;
    impl aw_agent_core::Tool for NoFallback {
        fn spec(&self) -> aw_agent_core::ToolSpec {
            panic!("unexpected ambient tool")
        }
        fn validate(&self, _: &serde_json::Value) -> Result<()> {
            panic!("unexpected ambient validation")
        }
        fn execute<'a>(
            &'a self,
            _: aw_agent_core::ToolContext,
            _: &'a serde_json::Value,
        ) -> aw_agent_core::ToolFuture<'a> {
            panic!("unexpected ambient execution")
        }
    }

    #[tokio::test]
    async fn native_source_route_uses_pinned_capability_and_original_receipt() {
        use crate::source_approval::{
            NativeSourceApproval, NativeSourceApprovalSpec, NativeSourceDirectorySpec,
        };
        #[cfg(not(feature = "git-source-effects"))]
        use crate::source_filesystem::{ApprovedSourceLayout, NativeSourceAdmission};
        use crate::source_filesystem::{NativeFilesystemDriver, SourceFilesystemRoute};
        #[cfg(feature = "git-source-effects")]
        use crate::source_git::GitNativeSourceHost;
        use aw_agent_core::{
            AgentExecutionBinding, Extension, HookContext, HookEvent, InvocationScope, ToolContext,
        };
        use aw_tools::{
            filesystem::{private_artifact, schema::Tool as FsTool},
            OperationId,
        };
        use aw_tools_adapter::{
            native_filesystem::{GrantSpec, PrivateArtifactRegistry},
            operation_router::OperationRouter,
        };

        // Pause exactly one real staged write inside its existing artifact
        // reservation boundary, so losing the awaiter is deterministic.
        struct StagePause {
            started: tokio::sync::oneshot::Sender<()>,
            resume: std::sync::mpsc::Receiver<()>,
        }
        #[derive(Default)]
        struct PausedArtifacts(std::sync::Mutex<Option<StagePause>>);
        impl private_artifact::Store for PausedArtifacts {
            fn durable(&self) -> bool {
                false
            }
            fn load(
                &self,
            ) -> std::result::Result<Vec<private_artifact::Record>, private_artifact::Error>
            {
                Ok(Vec::new())
            }
            fn reserve(
                &self,
                _: &private_artifact::Record,
            ) -> std::result::Result<(), private_artifact::Error> {
                if let Some(pause) = self.0.lock().unwrap().take() {
                    let _ = pause.started.send(());
                    pause
                        .resume
                        .recv()
                        .map_err(|_| private_artifact::Error::Conflict)?;
                }
                Ok(())
            }
            fn finalize(
                &self,
                _: &private_artifact::Record,
                _: &private_artifact::Record,
            ) -> std::result::Result<(), private_artifact::Error> {
                Ok(())
            }
            fn retire(
                &self,
                _: &private_artifact::Record,
            ) -> std::result::Result<(), private_artifact::Error> {
                Ok(())
            }
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = Temp(
            std::env::temp_dir().join(format!("aw-source-native-{}-{nonce}", std::process::id())),
        );
        let base = temp.0.join("source");
        std::fs::create_dir_all(base.join("code")).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join("code");
        let data = base.join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::write(root.join("code"), "initial").unwrap();
        std::fs::write(data.join("code"), "read-only data").unwrap();
        std::fs::create_dir(base.join("unapproved")).unwrap();
        std::fs::write(base.join("unapproved/code"), "unapproved secret").unwrap();
        let outside = temp.0.join("outside");
        std::fs::write(&outside, "protected").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let actor = PrincipalId::new("internal:fixture:native-source").unwrap();
        let registry = SourceDriverRegistry::new();
        #[cfg(not(feature = "git-source-effects"))]
        let bindings = {
            let source_driver = DriverKey::builtin("fixture-source").unwrap();
            let schema = SchemaSupport::new(1, 1).unwrap();
            registry
                .register(
                    source_driver.clone(),
                    SourceDriverSchemas {
                        binding: schema,
                        anchor: schema,
                        recovery: schema,
                    },
                    (),
                )
                .unwrap();
            [("code", &root), ("data", &data)].map(|(name, path)| SourceBinding {
                name: SourceName::new(name).unwrap(),
                driver: source_driver.clone(),
                binding_schema: 1,
                source_ref: format!("fixture:directory:{name}"),
                materialization: path.to_str().unwrap().into(),
                access: SourceAccess::ReadWrite,
                checkpoint_policy: CheckpointPolicy::Required,
            })
        };
        #[cfg(feature = "git-source-effects")]
        let (git_handle, bindings) = {
            let git = std::env::var_os("AW_GIT")
                .map(PathBuf::from)
                .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
            for directory in [&root, &data] {
                for args in [
                    &["init", "--initial-branch=main"][..],
                    &["add", "code"][..],
                    &[
                        "-c",
                        "user.name=Fixture",
                        "-c",
                        "user.email=fixture@example.invalid",
                        "-c",
                        "core.hooksPath=/dev/null",
                        "commit",
                        "--no-gpg-sign",
                        "-m",
                        "fixture",
                    ][..],
                ] {
                    let output = Command::new(&git)
                        .current_dir(directory)
                        .env("GIT_CONFIG_NOSYSTEM", "1")
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .args(args)
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "fixture Git: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            let handle = aw_git::source_driver::GitSourceDriver::new(git)
                .unwrap()
                .register(&registry)
                .unwrap();
            let driver = registry.reader().unwrap().get(&handle).unwrap();
            let bindings = [("code", &root), ("data", &data)].map(|(name, path)| {
                driver
                    .binding(
                        path,
                        SourceName::new(name).unwrap(),
                        SourceAccess::ReadWrite,
                        CheckpointPolicy::Required,
                    )
                    .unwrap()
            });
            (handle, bindings)
        };
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let layout = SessionBinding {
            working_directory: base.clone(),
            tool_root: base.clone(),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (session, branch) = store
            .create(&actor, "native source", layout.clone())
            .unwrap();
        let (reference, project): (String, String) = {
            let c = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            c.query_row(
                "SELECT '/d:'||domain_handle||'/i:'||identity_handle||'/p:'||project_handle||'/a:'||agent_handle,
                        '/d:'||domain_handle||'/i:'||identity_handle||'/p:'||project_handle
                 FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
                params![session.id,branch.id], |row| Ok((row.get(0)?,row.get(1)?)),
            ).unwrap()
        };
        let sandbox = store
            .sandbox_create_fresh_with_sources(&actor, &project, &layout, &bindings)
            .unwrap();
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
                prompt: "native source fixture".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        let mut records = vec![
            (
                "native:read",
                FsTool::Read,
                ToolEffect::Read,
                serde_json::json!({"path":"code/code"}),
            ),
            (
                "native:data-read",
                FsTool::Read,
                ToolEffect::Read,
                serde_json::json!({"path":"data/code"}),
            ),
            (
                "native:unapproved",
                FsTool::Read,
                ToolEffect::Read,
                serde_json::json!({"path":"unapproved/code"}),
            ),
            (
                "native:escape",
                FsTool::Read,
                ToolEffect::Read,
                serde_json::json!({"path":"code/escape"}),
            ),
            (
                "native:parent",
                FsTool::Read,
                ToolEffect::Read,
                serde_json::json!({"path":"."}),
            ),
        ];
        #[cfg(feature = "git-source-effects")]
        for (id, path) in [
            ("native:git-private", "code/.git/config"),
            ("native:data-private", "data/.git/config"),
        ] {
            records.push((
                id,
                FsTool::Read,
                ToolEffect::Read,
                serde_json::json!({"path":path}),
            ));
        }
        records.extend([
            (
                "native:data-write",
                FsTool::Write,
                ToolEffect::Write,
                serde_json::json!({"path":"data/code","content":"must not write"}),
            ),
            (
                "native:write",
                FsTool::Write,
                ToolEffect::Write,
                serde_json::json!({"path":"code/code","content":"native changed"}),
            ),
            (
                "native:detached",
                FsTool::Write,
                ToolEffect::Write,
                serde_json::json!({"path":"code/code","content":"detached changed"}),
            ),
        ]);
        let records: Vec<_> = records
            .into_iter()
            .map(|(id, tool, effect, arguments)| ToolRecord {
                call: ToolCall {
                    id: id.into(),
                    name: OperationId::Fs(tool).model_name().into(),
                    arguments,
                },
                effect,
                execution_site: "local".into(),
                status: ToolStatus::Running,
                output: None,
            })
            .collect();
        let mut assistant = Message::text(1025, Role::Assistant, "");
        assistant.tool_calls = records.iter().map(|r| r.call.clone()).collect();
        journal
            .append(1, JournalEntry::Assistant { message: assistant })
            .await
            .unwrap();
        for record in &records {
            journal
                .append(
                    1,
                    JournalEntry::ToolStarted {
                        record: record.clone(),
                        source: Some(ToolSourceEvidence::trusted_host()),
                    },
                )
                .await
                .unwrap();
        }
        // Fixture-only association after ordinary journal admission.
        {
            let c = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            c.execute("UPDATE agent_state_current SET sandbox_id=?1,admission='held' WHERE session_id=?2 AND branch_id=?3",
                params![sandbox.canonical_ref, session.id, branch.id]).unwrap();
        }
        let artifacts = Arc::new(PausedArtifacts::default());
        let approval = NativeSourceApproval::pin(
            store.clone(),
            NativeSourceApprovalSpec {
                agent_ref: reference.clone(),
                actor: actor.clone(),
                policy_ref: "host:fixture-native".into(),
                authority_ref: "host:fixture-native-authority".into(),
                authority_generation: 1,
                base: base.clone(),
                // Reverse input order deliberately; the policy uses canonical
                // name order and retains each original artifact owner.
                directories: vec![
                    NativeSourceDirectorySpec {
                        source: VersionedSourceBinding {
                            binding: bindings[1].clone(),
                            binding_rev: 1,
                        },
                        directory: GrantSpec::read(&data),
                        private_artifacts: PrivateArtifactRegistry::memory(),
                    },
                    NativeSourceDirectorySpec {
                        source: VersionedSourceBinding {
                            binding: bindings[0].clone(),
                            binding_rev: 1,
                        },
                        directory: GrantSpec::read_write(&root),
                        private_artifacts: PrivateArtifactRegistry::open(artifacts.clone())
                            .unwrap(),
                    },
                ],
            },
        )
        .unwrap();
        let set = SourceMutationSet::from_bindings(&bindings).unwrap();
        let agent_binding =
            AgentExecutionBinding::from_trusted_host(&reference, &session.id, &branch.id).unwrap();
        // Physical approval and Agent access alone cannot mint a Store grant.
        assert!(approval.preflight(&store, &actor, &agent_binding).is_err());
        let drivers = SandboxDriverRegistry::new();
        let handle = drivers
            .register(
                DriverKey::builtin("native-filesystem").unwrap(),
                NativeFilesystemDriver,
            )
            .unwrap();
        #[cfg(feature = "git-source-effects")]
        let host = GitNativeSourceHost::new(
            approval.clone(),
            git_handle,
            drivers.clone(),
            handle.clone(),
        )
        .unwrap();
        #[cfg(feature = "git-source-effects")]
        assert_eq!(host.publish_grant(&actor, 0).unwrap().revision, 1);
        #[cfg(not(feature = "git-source-effects"))]
        assert_eq!(approval.publish_grant(&actor, 0).unwrap().revision, 1);
        assert_eq!(
            approval
                .preflight(&store, &actor, &agent_binding)
                .unwrap()
                .sources[0]
                .access,
            SourceAccess::ReadWrite
        );
        assert!(approval
            .preflight(&SessionStore::in_memory().unwrap(), &actor, &agent_binding)
            .is_err());
        let wrong =
            AgentExecutionBinding::from_trusted_host(&reference, "other-session", &branch.id)
                .unwrap();
        assert!(approval.preflight(&store, &actor, &wrong).is_err());

        // No ambient root or private-artifact reconciliation during
        // registration, even when the policy has already been approved.
        let ambient = temp.0.join("unapproved-root");
        let mut options = LocalToolOptions::new(&ambient, ToolMode::Workspace);
        options.selected_tools = vec!["read".into(), "write".into()];
        let builder = Agent::builder(
            Arc::new(NoInference),
            AgentOptions::new("fixture", "fixture"),
        );
        #[cfg(feature = "git-source-effects")]
        {
            // The concrete host activation composition uses the same
            // original tool registry and the same native-only builder.
            let mut explicit = options.clone();
            explicit.operation_router = Arc::new(Default::default());
            let configured = host
                .configure(
                    &store,
                    actor.clone(),
                    agent_binding.clone(),
                    Agent::builder(
                        Arc::new(NoInference),
                        AgentOptions::new("fixture", "fixture"),
                    ),
                    &explicit,
                )
                .unwrap()
                .build()
                .unwrap();
            assert_eq!(configured.registered_tool_names(), ["read", "write"]);
            assert!(!explicit.operation_router.is_installed().unwrap());
            assert!(configured
                .registered_extension_names()
                .iter()
                .any(|name| name == "host-source-filesystem"));
            explicit.selected_tools.push("bash".into());
            assert!(host
                .preflight(&store, &actor, &agent_binding, &explicit)
                .is_err());
        }
        #[cfg(feature = "git-source-effects")]
        let resolve = {
            let host = host.clone();
            move |ctx: &ToolContext, tool: FsTool| host.resolve(ctx, tool)
        };
        #[cfg(not(feature = "git-source-effects"))]
        let resolve = {
            let approval = approval.clone();
            move |_: &ToolContext, tool: FsTool| {
                let authority = approval.read()?;
                let mut roots = authority.roots().to_vec();
                if tool.action() == aw_tools::filesystem::access::Action::Read {
                    for root in &mut roots {
                        root.source.access = SourceAccess::ReadOnly;
                    }
                }
                let layout =
                    ApprovedSourceLayout::pin(authority.base(), roots, std::iter::empty())?;
                Ok(NativeSourceAdmission { authority, layout })
            }
        };
        let route = Arc::new(
            SourceFilesystemRoute::new(
                store.clone(),
                agent_binding.clone(),
                drivers,
                handle,
                builder.tool_binding_authority(),
                options.limits,
                resolve,
            )
            .unwrap(),
        );
        let agent = route
            .clone()
            .configure(builder, actor.clone(), &options)
            .unwrap()
            .build()
            .unwrap();
        assert!(!ambient.exists());
        let mut final_records = Vec::new();
        for mut record in records {
            let id = record.call.id.clone();
            let ctx = ToolContext {
                actor: actor.clone(),
                turn_id: 1,
                call_id: id.clone(),
                source: ToolSource::trusted_host(),
                effect: record.effect.clone(),
                execution_site: "local".into(),
                cancellation: CancellationToken::new(),
                result_budget_bytes: options.limits.result_bytes,
                identity: identity.clone(),
                scope: Some(InvocationScope {
                    session_id: session.id.clone(),
                    branch_id: branch.id.clone(),
                }),
                execution_owner: Some(agent_binding.clone()),
                execution_scope: None,
                live_step_gate: None,
            };
            let op = OperationId::from_model_name(&record.call.name).unwrap();
            if id == "native:read" {
                assert!(route
                    .execute(
                        op,
                        ctx.clone(),
                        &serde_json::json!({"path":"code/escape"}),
                        Arc::new(NoFallback)
                    )
                    .await
                    .is_err());
            }
            if id == "native:detached" {
                // The preceding completed native write missed its hook.
                // Consume the existing positive fact, not another effect.
                assert_eq!(
                    store
                        .sandbox_get(&actor, &sandbox.canonical_ref)
                        .unwrap()
                        .rev,
                    sandbox.rev + 1
                );
                assert_eq!(
                    store
                        .sandbox_recover_tool_source_effect(
                            &actor,
                            &reference,
                            1,
                            "native:write",
                            |_| panic!("recorded native proof cannot need reinspection")
                        )
                        .unwrap(),
                    SandboxEffectOutcome::Settled
                );

                let (started, entered) = tokio::sync::oneshot::channel();
                let (resume, wait) = std::sync::mpsc::channel();
                *artifacts.0.lock().unwrap() = Some(StagePause {
                    started,
                    resume: wait,
                });
                let mut pending =
                    route.execute(op, ctx, &record.call.arguments, Arc::new(NoFallback));
                tokio::select! {
                    result = &mut pending => panic!("native worker finished before pause: {result:?}"),
                    entered = tokio::time::timeout(Duration::from_secs(5), entered) => entered.unwrap().unwrap(),
                }
                drop(pending);
                assert_eq!(std::fs::read(root.join("code")).unwrap(), b"native changed");
                // Every exposed root is claimed, including the read-only
                // data source while the writable code worker is detached.
                for binding in &bindings {
                    let source =
                        SourceMutationSet::from_bindings(std::slice::from_ref(binding)).unwrap();
                    assert!(store
                        .acquire_source_mutation_barrier(&source, "host:detached-active")
                        .is_err());
                }
                let revoke = approval.revoke();
                tokio::pin!(revoke);
                tokio::select! {
                    biased;
                    _ = &mut revoke => panic!("withdrawal released a live detached worker"),
                    _ = std::future::ready(()) => {},
                }
                assert!(approval.read().is_err());
                resume.send(()).unwrap();
                assert!(tokio::time::timeout(Duration::from_secs(5), &mut revoke)
                    .await
                    .unwrap());
                assert_eq!(
                    std::fs::read(root.join("code")).unwrap(),
                    b"detached changed"
                );
                // Guard release follows durable worker quiescence. There
                // is still no ToolFinished for the dropped async result.
                assert!(store
                    .acquire_source_mutation_barrier(&set, "host:detached-unrecorded")
                    .is_err());
                assert!(store
                    .sandbox_recover_tool_source_effect(&actor, &reference, 1, &id, |_| panic!(
                        "unfinished Run cannot release the source"
                    ))
                    .is_err());
                record.status = ToolStatus::Interrupted;
                final_records.push(record);
                continue;
            }
            let output = route
                .execute(op, ctx, &record.call.arguments, Arc::new(NoFallback))
                .await
                .unwrap();
            match id.as_str() {
                "native:read" => assert!(output.content.contains("initial") && !output.is_error),
                "native:data-read" => {
                    assert!(output.content.contains("read-only data") && !output.is_error)
                }
                "native:escape" => {
                    assert!(output.is_error && !output.content.contains("protected"))
                }
                "native:unapproved" | "native:parent" => {
                    assert!(output.is_error && !output.content.contains("unapproved secret"))
                }
                "native:data-write" => {
                    assert!(output.is_error);
                    assert_eq!(std::fs::read(data.join("code")).unwrap(), b"read-only data");
                }
                #[cfg(feature = "git-source-effects")]
                "native:git-private" | "native:data-private" => {
                    assert!(output.is_error && !output.content.contains("repositoryformatversion"))
                }
                "native:write" => assert!(
                    !output.is_error
                        && std::fs::read(root.join("code")).unwrap() == b"native changed"
                ),
                _ => unreachable!(),
            }
            assert!(approval.read().is_ok());
            for binding in &bindings {
                let source =
                    SourceMutationSet::from_bindings(std::slice::from_ref(binding)).unwrap();
                assert!(store
                    .acquire_source_mutation_barrier(&source, "host:before-receipt")
                    .is_err());
            }
            assert!(store
                .sandbox_settle_recorded_tool_source_effect(&actor, &reference, 1, &id)
                .is_err());
            record.status = if output.is_error {
                ToolStatus::Failed
            } else {
                ToolStatus::Succeeded
            };
            record.output = Some(output.clone());
            journal
                .append(
                    1,
                    JournalEntry::ToolFinished {
                        record: record.clone(),
                    },
                )
                .await
                .unwrap();
            if id != "native:write" {
                // Real original post-journal hook; no extra execution or
                // model-visible result projection.
                route
                    .on_hook(
                        HookContext {
                            turn_id: 1,
                            continuation: None,
                            profile: "fixture",
                            model: "fixture",
                            identity: &identity,
                        },
                        HookEvent::ToolResult {
                            call: record.call.clone(),
                            receipt: output.clone(),
                            visible: output,
                        },
                    )
                    .await
                    .unwrap();
                store
                    .acquire_source_mutation_barrier(&set, "host:after-receipt")
                    .unwrap()
                    .settle(SourceMutationOutcome::Settled)
                    .unwrap();
            }
            final_records.push(record);
        }
        // The detached write physically finished and recorded its own
        // quiescence, but the Sandbox revision awaits original Run closure.
        assert_eq!(
            store
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev + 2
        );
        assert_eq!(std::fs::read(&outside).unwrap(), b"protected");
        assert!(approval.read().is_err());
        assert!(approval.publish_grant(&actor, 1).is_err());
        assert!(
            store
                .sandbox_grant_status(&actor, &reference)
                .unwrap()
                .active
        );
        assert!(approval.preflight(&store, &actor, &agent_binding).is_err());
        #[cfg(feature = "git-source-effects")]
        assert!(host
            .preflight(&store, &actor, &agent_binding, &options)
            .is_err());
        // No synthesized ToolFinished: the original journal owner recovers
        // this lost Run on reopen.
        drop(agent);
        drop(options);
        drop(route);
        #[cfg(feature = "git-source-effects")]
        drop(host);
        drop(approval);
        drop(journal);
        drop(store);
        let store = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        let recovered_turn: Turn = {
            let c = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            let payload: String = c.query_row(
                "SELECT n.payload FROM runs r JOIN nodes n ON n.id=r.final_node WHERE r.session_id=?1 AND r.turn_id=1 AND r.status='interrupted'",
                [&session.id], |row| row.get(0)
            ).unwrap();
            serde_json::from_str(&payload).unwrap()
        };
        assert_eq!(
            serde_json::to_value(&recovered_turn.tool_records).unwrap(),
            serde_json::to_value(&final_records).unwrap()
        );
        assert!(recovered_turn.needs_reconciliation);
        for id in ["native:write", "native:detached"] {
            assert_eq!(
                store
                    .sandbox_recover_tool_source_effect(&actor, &reference, 1, id, |_| panic!(
                        "recorded quiescence needs no external reinspection"
                    ))
                    .unwrap(),
                SandboxEffectOutcome::Settled
            );
        }
        assert_eq!(
            store
                .sandbox_settle_recorded_tool_source_effect(&actor, &reference, 1, "native:write")
                .unwrap(),
            Some(SandboxEffectOutcome::Settled)
        );
        assert_eq!(
            store
                .sandbox_get(&actor, &sandbox.canonical_ref)
                .unwrap()
                .rev,
            sandbox.rev + 3
        );
        assert!(store.activate(&actor, &session.id, &branch.id).is_err());
    }
}
