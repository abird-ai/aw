//! One replay-stable admission guardian over the existing tool-free auxiliary executor.
//!
//! This module never grants an operation. The caller must first resolve exact
//! positive authority with aw-policy and still revalidate ToolPolicy/Envelope/
//! resource/payer authority when consuming the durable decision.

use aw_agent_core::{
    tool::ToolBindingAuthority, AuxiliaryExecutor, AuxiliaryInput, Error, Outcome, PrincipalId,
    ProviderSelection, Result,
};
use aw_host::AuthenticatedToolGrantEvidence;
use aw_policy::{AuthorizedPolicy, Decision, PolicyDecision, Strategy};
use aw_store::{
    PolicyAdmissionFact, PolicyAdmissionRequest, PolicyAdmissionState, PolicyGuardianJournal,
    SessionStore,
};
use aw_tools::remote_start::{RemoteStartAuthorization, RemoteStartIntent, RemoteStartSource};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::future::Future;

const MAX_GUARDIAN_OUTPUT: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireDecision {
    Allow,
    Deny,
    Ask,
    Narrow,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireRoute {
    Local,
    DelegateAgent,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GuardianDecisionWire {
    decision: WireDecision,
    #[serde(default)]
    route: Option<WireRoute>,
    #[serde(default)]
    approval_scope: Option<String>,
    #[serde(default)]
    narrow_ref: Option<String>,
}

fn terminal_or_waiting(state: PolicyAdmissionState) -> bool {
    matches!(
        state,
        PolicyAdmissionState::Allowed
            | PolicyAdmissionState::Denied
            | PolicyAdmissionState::AskPending
            | PolicyAdmissionState::AskApproved
            | PolicyAdmissionState::AskDenied
            | PolicyAdmissionState::AskExpired
            | PolicyAdmissionState::Narrowed
            | PolicyAdmissionState::Consumed
    )
}

fn route(value: Option<WireRoute>) -> Option<Strategy> {
    value.map(|route| match route {
        WireRoute::Local => Strategy::Local,
        WireRoute::DelegateAgent => Strategy::DelegateAgent,
    })
}

fn decision(value: WireDecision) -> Decision {
    match value {
        WireDecision::Allow => Decision::Allow,
        WireDecision::Deny => Decision::Deny,
        WireDecision::Ask => Decision::Ask,
        WireDecision::Narrow => Decision::Narrow,
    }
}

fn decode_sha256(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Denied("invalid remote Start authority digest"));
    }
    let mut out = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or(Error::Denied("invalid remote Start authority digest"))?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or(Error::Denied("invalid remote Start authority digest"))?;
        out[index] = ((high << 4) | low) as u8;
    }
    Ok(out)
}

/// Commit one source-fenced remote Start under the exact current ToolBinding.
/// Exact replay lookup happens first so a committed attempt remains the same
/// attempt after later revocation; changed intent can never mint a replacement.
pub fn authorize_remote_start(
    store: &SessionStore,
    authority: &ToolBindingAuthority,
    actor: &PrincipalId,
    evidence: &AuthenticatedToolGrantEvidence,
    session: &str,
    policy_admission_id: &str,
    intent: RemoteStartIntent,
) -> Result<RemoteStartAuthorization> {
    if let Some(existing) = store.remote_start_lookup(actor, session, &intent)? {
        return Ok(existing);
    }

    let operation = evidence.operation_context();
    if operation.operation != intent.operation
        || operation.resource_scope != intent.canonical_ref
        || evidence.binding().source.epoch != operation.source_epoch
        || evidence.binding().execution_site != operation.site
    {
        return Err(Error::Denied(
            "remote Start differs from the exact authenticated ToolBinding",
        ));
    }
    let source_kind = match operation.source_kind {
        aw_policy::SourceKind::Host => "host",
        aw_policy::SourceKind::Mcp => "mcp",
        aw_policy::SourceKind::Extension => "extension",
    };
    let source = RemoteStartSource {
        source_kind: source_kind.into(),
        source_id: operation.source_id.to_owned(),
        source_epoch: operation.source_epoch,
        site: operation.site.to_owned(),
        resource_scope: operation.resource_scope.to_owned(),
        grant_sha256: decode_sha256(&evidence.authority_sha256())?,
    };

    authority.with_revalidated_binding(actor, evidence.binding(), || {
        store.remote_start_authorize(actor, session, policy_admission_id, intent, source)
    })
}

/// Inputs for one already-authorized guardian attempt. Grouped as one value so
/// the call site makes the authority tuple explicit without a wide positional
/// parameter list.
pub struct AdmissionGuardianRequest<'a> {
    pub store: &'a SessionStore,
    pub actor: &'a PrincipalId,
    pub request: PolicyAdmissionRequest,
    pub authorized: AuthorizedPolicy,
    pub selected: &'a ProviderSelection,
    pub input: &'a AuxiliaryInput,
    pub decision_revision: u64,
}

/// Execute at most one guardian inference for one already-authorized operation.
/// The original policy claim and all approval/provider/payer evidence must be
/// prepared before this function is called.
pub async fn run_admission_guardian<C>(
    admission: AdmissionGuardianRequest<'_>,
    cancel: C,
) -> Result<PolicyAdmissionFact>
where
    C: Future<Output = ()> + Send,
{
    let AdmissionGuardianRequest {
        store,
        actor,
        request,
        authorized,
        selected,
        input,
        decision_revision,
    } = admission;
    if decision_revision == 0 {
        return Err(Error::Denied("policy decision revision is invalid"));
    }
    if !request.guardian_required {
        return Err(Error::Denied(
            "admission guardian requires a pre-reserved guardian attempt",
        ));
    }
    if input.max_output_bytes == 0 || input.max_output_bytes > MAX_GUARDIAN_OUTPUT {
        return Err(Error::Limit(
            "policy guardian output must be bounded to 1..8192 bytes",
        ));
    }

    let effective = authorized.effective();
    if request.candidate_local != effective.routes().contains(Strategy::Local)
        || request.candidate_delegate_agent != effective.routes().contains(Strategy::DelegateAgent)
    {
        return Err(Error::Denied(
            "policy admission candidate routes differ from resolved authority",
        ));
    }
    selected.revalidate()?;
    if request.guardian_identity.as_ref() != Some(selected.identity())
        || request.guardian_site.as_deref() != Some(selected.identity().inference_executor.as_str())
    {
        return Err(Error::Denied(
            "guardian provider/payer/site differs from the captured admission",
        ));
    }

    let claimed = store.policy_admission_claim(actor, request)?;
    if terminal_or_waiting(claimed.state) {
        return Ok(claimed);
    }
    match claimed.state {
        PolicyAdmissionState::Claimed => {}
        PolicyAdmissionState::GuardianStarted
        | PolicyAdmissionState::GuardianUnknown
        | PolicyAdmissionState::GuardianSettled => {
            return Err(Error::Conflict(
                "guardian attempt already started/settled ambiguously; inspect, do not replay",
            ))
        }
        _ => unreachable!("terminal states handled above"),
    }

    let journal = PolicyGuardianJournal::new(
        store.clone(),
        actor.clone(),
        claimed.request.session_id.clone(),
        claimed.id.clone(),
    );
    let result = AuxiliaryExecutor
        .execute(selected, input, &journal, cancel)
        .await?;
    if result.outcome != Outcome::Completed {
        return Err(Error::Conflict(
            "policy guardian did not yield a complete decision; inspect, do not replay",
        ));
    }

    let parsed: GuardianDecisionWire = serde_json::from_str(result.text.trim())
        .map_err(|_| Error::Protocol("policy guardian returned invalid decision JSON"))?;
    let route = route(parsed.route);
    let policy_decision = PolicyDecision {
        decision: decision(parsed.decision),
        revision: decision_revision,
        route,
        approval_scope: parsed.approval_scope.as_deref(),
        narrow_ref: parsed.narrow_ref.as_deref(),
    };
    policy_decision
        .validate(effective)
        .map_err(|_| Error::Denied("guardian decision exceeds the resolved policy envelope"))?;

    let digest = format!("{:x}", Sha256::digest(result.text.as_bytes()));
    store.policy_admission_record_decision(
        actor,
        &claimed.request.session_id,
        &claimed.id,
        policy_decision,
        &digest,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    struct Temp(std::path::PathBuf);

    #[cfg(unix)]
    impl Temp {
        fn new() -> Self {
            use std::os::unix::fs::DirBuilderExt;
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "aw-policy-runtime-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .unwrap();
            Self(path)
        }
    }

    #[cfg(unix)]
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    struct GuardianProvider {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[cfg(unix)]
    impl aw_agent_core::Provider for GuardianProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [aw_agent_core::Message],
            _model: &'a str,
            sink: &'a mut aw_agent_core::DeltaSink<'_>,
        ) -> aw_agent_core::CompletionFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                sink(r#"{"decision":"allow","route":"local"}"#)?;
                Ok(aw_agent_core::CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: aw_agent_core::Usage {
                        input_tokens: Some(11),
                        output_tokens: Some(7),
                    },
                })
            })
        }
    }

    #[cfg(unix)]
    struct RemoteBashTool;

    #[cfg(unix)]
    impl aw_agent_core::Tool for RemoteBashTool {
        fn spec(&self) -> aw_agent_core::ToolSpec {
            aw_agent_core::ToolSpec {
                name: "bash".into(),
                description: "remote Start fixture".into(),
                parameters: serde_json::json!({"type":"object"}),
                effect: aw_agent_core::ToolEffect::Process,
            }
        }

        fn validate(&self, _arguments: &serde_json::Value) -> Result<()> {
            Ok(())
        }

        fn execute<'a>(
            &'a self,
            _ctx: aw_agent_core::ToolContext,
            _arguments: &'a serde_json::Value,
        ) -> aw_agent_core::ToolFuture<'a> {
            Box::pin(async { Ok(aw_agent_core::ToolOutput::text("unused")) })
        }
    }

    #[cfg(unix)]
    fn session_binding() -> aw_store::SessionBinding {
        aw_store::SessionBinding {
            working_directory: std::path::PathBuf::from("/tmp"),
            tool_root: std::path::PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn completed_guardian_replay_never_pays_twice_or_accepts_changed_policy() {
        use aw_agent_core::{Agent, AgentOptions, TurnJournal, TurnStart};
        use aw_policy::{
            AuthenticatedGrantPath, Effect, GrantRestrictions, GrantSubject, OperationContext,
            PolicyContext, SourceKind,
        };
        use std::{
            future::pending,
            sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            },
            time::Duration,
        };

        let temp = Temp::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(GuardianProvider {
            calls: calls.clone(),
        });
        let mut agent = Agent::builder(provider, AgentOptions::new("fixture", "model"))
            .build()
            .unwrap();
        let actor = agent.actor().clone();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(&actor, "policy guardian", session_binding())
            .unwrap();
        let (view, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle.clone())))
            .unwrap();
        let identity = agent.default_identity().unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "gated operation".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        let writer_epoch = store
            .workspace_writer(&actor, None, &session.workspace_id)
            .unwrap()
            .writer_epoch;
        let selected = agent
            .provider_accounts()
            .select(&actor, &identity.account_id, &identity.model)
            .unwrap();

        let operation = OperationContext {
            operation: "tools.fs.read",
            source_kind: SourceKind::Host,
            source_id: "trusted-host",
            source_epoch: 1,
            effect: Effect::Read,
            site: "local",
            target_domain: "d:local",
            resource_scope: "/d:local/i:owner/p:repo",
        };
        let context = PolicyContext {
            actor: actor.as_str(),
            domain: "d:local",
            active_team: None,
            workspace: "/d:local/i:owner/p:repo",
            session: Some("/d:local/i:local/p:repo/a:0"),
            graph: None,
            node: None,
            invocation: "turn:1/call:policy-guardian",
            operation,
        };
        let grant = AuthenticatedGrantPath {
            issuer: "host:tool-binding",
            issuer_authority_epoch: 1,
            grant_scope: "/d:local/i:owner/p:repo",
            proof_ref: "grant-read-v1",
            subject: GrantSubject::DirectIdentity {
                identity: actor.as_str(),
            },
            restrictions: GrantRestrictions {
                operation: operation.operation,
                source_kind: operation.source_kind,
                source_id: operation.source_id,
                source_epoch: operation.source_epoch,
                effect: operation.effect,
                site: operation.site,
                target_domain: operation.target_domain,
                resource_scope: operation.resource_scope,
            },
        };
        let authorized = aw_policy::resolve_authorized(&context, &[grant], &[]).unwrap();

        let request = PolicyAdmissionRequest {
            session_id: session.id.clone(),
            branch_id: branch.id.clone(),
            writer_epoch,
            turn_id: 1,
            call_id: "policy-guardian".into(),
            canonical_target: "/d:local/i:local/p:repo/a:0/t:1/j:read".into(),
            request_sha256: "a".repeat(64),
            policy_revision_sha256: "b".repeat(64),
            authority_sha256: "c".repeat(64),
            candidate_local: authorized.effective().routes().contains(Strategy::Local),
            candidate_delegate_agent: authorized
                .effective()
                .routes()
                .contains(Strategy::DelegateAgent),
            guardian_required: true,
            guardian_identity: Some(identity.clone()),
            guardian_site: Some(identity.inference_executor.clone()),
            approval_scope: None,
            authorized_approvers: Vec::new(),
            approval_expires_at: None,
        };
        let input = AuxiliaryInput {
            kind: aw_agent_core::AuxiliaryKind::Side,
            messages: agent
                .auxiliary_messages(&selected, "Return the JSON admission decision only.")
                .await
                .unwrap(),
            max_output_bytes: 1024,
            timeout: Duration::from_secs(5),
        };

        let first = run_admission_guardian(
            AdmissionGuardianRequest {
                store: &store,
                actor: &actor,
                request: request.clone(),
                authorized,
                selected: &selected,
                input: &input,
                decision_revision: 4,
            },
            pending::<()>(),
        )
        .await
        .unwrap();
        assert_eq!(first.state, PolicyAdmissionState::Allowed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let replay = run_admission_guardian(
            AdmissionGuardianRequest {
                store: &store,
                actor: &actor,
                request: request.clone(),
                authorized,
                selected: &selected,
                input: &input,
                decision_revision: 4,
            },
            pending::<()>(),
        )
        .await
        .unwrap();
        assert_eq!(replay, first);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "matching replay must never invoke the guardian again"
        );

        let mut changed = request;
        changed.policy_revision_sha256 = "d".repeat(64);
        assert!(matches!(
            run_admission_guardian(
                AdmissionGuardianRequest {
                    store: &store,
                    actor: &actor,
                    request: changed,
                    authorized,
                    selected: &selected,
                    input: &input,
                    decision_revision: 4,
                },
                pending::<()>(),
            )
            .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn started_guardian_without_decision_never_replays_or_reclaims_under_new_policy() {
        use aw_agent_core::{Agent, AgentOptions, TurnJournal, TurnStart};
        use aw_policy::{
            AuthenticatedGrantPath, Effect, GrantRestrictions, GrantSubject, OperationContext,
            PolicyContext, SourceKind,
        };
        use std::{
            future::pending,
            sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            },
            time::Duration,
        };

        let temp = Temp::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(GuardianProvider {
            calls: calls.clone(),
        });
        let mut agent = Agent::builder(provider, AgentOptions::new("fixture", "model"))
            .build()
            .unwrap();
        let actor = agent.actor().clone();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(&actor, "policy guardian crash", session_binding())
            .unwrap();
        let (view, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle.clone())))
            .unwrap();
        let identity = agent.default_identity().unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "gated operation".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        let writer_epoch = store
            .workspace_writer(&actor, None, &session.workspace_id)
            .unwrap()
            .writer_epoch;
        let selected = agent
            .provider_accounts()
            .select(&actor, &identity.account_id, &identity.model)
            .unwrap();

        let operation = OperationContext {
            operation: "tools.fs.read",
            source_kind: SourceKind::Host,
            source_id: "trusted-host",
            source_epoch: 1,
            effect: Effect::Read,
            site: "local",
            target_domain: "d:local",
            resource_scope: "/d:local/i:owner/p:repo",
        };
        let context = PolicyContext {
            actor: actor.as_str(),
            domain: "d:local",
            active_team: None,
            workspace: "/d:local/i:owner/p:repo",
            session: Some("/d:local/i:local/p:repo/a:0"),
            graph: None,
            node: None,
            invocation: "turn:1/call:policy-guardian-crash",
            operation,
        };
        let grant = AuthenticatedGrantPath {
            issuer: "host:tool-binding",
            issuer_authority_epoch: 1,
            grant_scope: "/d:local/i:owner/p:repo",
            proof_ref: "grant-read-crash-v1",
            subject: GrantSubject::DirectIdentity {
                identity: actor.as_str(),
            },
            restrictions: GrantRestrictions {
                operation: operation.operation,
                source_kind: operation.source_kind,
                source_id: operation.source_id,
                source_epoch: operation.source_epoch,
                effect: operation.effect,
                site: operation.site,
                target_domain: operation.target_domain,
                resource_scope: operation.resource_scope,
            },
        };
        let authorized = aw_policy::resolve_authorized(&context, &[grant], &[]).unwrap();
        let request = PolicyAdmissionRequest {
            session_id: session.id.clone(),
            branch_id: branch.id.clone(),
            writer_epoch,
            turn_id: 1,
            call_id: "policy-guardian-crash".into(),
            canonical_target: "/d:local/i:local/p:repo/a:0/t:1/j:read-crash".into(),
            request_sha256: "1".repeat(64),
            policy_revision_sha256: "2".repeat(64),
            authority_sha256: "3".repeat(64),
            candidate_local: authorized.effective().routes().contains(Strategy::Local),
            candidate_delegate_agent: authorized
                .effective()
                .routes()
                .contains(Strategy::DelegateAgent),
            guardian_required: true,
            guardian_identity: Some(identity.clone()),
            guardian_site: Some(identity.inference_executor.clone()),
            approval_scope: None,
            authorized_approvers: Vec::new(),
            approval_expires_at: None,
        };
        let input = AuxiliaryInput {
            kind: aw_agent_core::AuxiliaryKind::Side,
            messages: agent
                .auxiliary_messages(&selected, "Return the JSON admission decision only.")
                .await
                .unwrap(),
            max_output_bytes: 1024,
            timeout: Duration::from_secs(5),
        };

        let claimed = store
            .policy_admission_claim(&actor, request.clone())
            .unwrap();
        let digest = input.digest().unwrap();
        let started = store
            .policy_admission_start_guardian(&actor, &session.id, &claimed.id, &digest)
            .unwrap();
        assert_eq!(started.state, PolicyAdmissionState::GuardianStarted);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        assert!(matches!(
            run_admission_guardian(
                AdmissionGuardianRequest {
                    store: &store,
                    actor: &actor,
                    request: request.clone(),
                    authorized,
                    selected: &selected,
                    input: &input,
                    decision_revision: 4,
                },
                pending::<()>(),
            )
            .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a durable started attempt must never dispatch another guardian on replay"
        );

        let mut changed = request;
        changed.policy_revision_sha256 = "4".repeat(64);
        assert!(matches!(
            run_admission_guardian(
                AdmissionGuardianRequest {
                    store: &store,
                    actor: &actor,
                    request: changed,
                    authorized,
                    selected: &selected,
                    input: &input,
                    decision_revision: 4,
                },
                pending::<()>(),
            )
            .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "policy-generation change must not turn the original replay identity into new paid work"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_start_exact_replay_survives_tool_revoke_but_fresh_attempt_denies() {
        use aw_agent_core::{Agent, AgentOptions, ToolSource, TurnJournal, TurnStart};
        use aw_tools::remote_start::{
            RemoteRuntimeLease, RemoteSessionAction, RemoteStartIntent, REMOTE_START_VERSION,
        };
        use std::sync::{atomic::AtomicUsize, Arc};

        let temp = Temp::new();
        let provider = Arc::new(GuardianProvider {
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let mut agent = Agent::builder(provider, AgentOptions::new("fixture", "model"))
            .build()
            .unwrap();
        agent.register_host_tool(Arc::new(RemoteBashTool)).unwrap();
        let actor = agent.actor().clone();
        let authority = agent.tool_binding_authority();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(&actor, "remote Start", session_binding())
            .unwrap();
        let (view, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(Arc::new(handle.clone())))
            .unwrap();
        let identity = agent.default_identity().unwrap();
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "remote".into(),
                identity,
            })
            .await
            .unwrap();

        let canonical = "/d:local/i:owner/p:repo/a:0";
        let evidence = AuthenticatedToolGrantEvidence::capture(
            &authority,
            &actor,
            "bash",
            "bash",
            "d:local",
            canonical,
            "/d:local/i:owner/p:repo",
        )
        .unwrap();
        let writer_epoch = store
            .workspace_writer(&actor, None, &session.workspace_id)
            .unwrap()
            .writer_epoch;
        let request_digest = [2u8; 32];
        let request = PolicyAdmissionRequest {
            session_id: session.id.clone(),
            branch_id: branch.id.clone(),
            writer_epoch,
            turn_id: 1,
            call_id: "remote-start".into(),
            canonical_target: canonical.into(),
            request_sha256: "02".repeat(32),
            policy_revision_sha256: "04".repeat(32),
            authority_sha256: evidence.authority_sha256(),
            candidate_local: true,
            candidate_delegate_agent: false,
            guardian_required: false,
            guardian_identity: None,
            guardian_site: None,
            approval_scope: None,
            authorized_approvers: Vec::new(),
            approval_expires_at: None,
        };
        let claimed = store.policy_admission_claim(&actor, request).unwrap();
        let decided = store
            .policy_admission_record_decision(
                &actor,
                &session.id,
                &claimed.id,
                PolicyDecision {
                    decision: Decision::Allow,
                    revision: 1,
                    route: Some(Strategy::Local),
                    approval_scope: None,
                    narrow_ref: None,
                },
                &"08".repeat(32),
            )
            .unwrap();
        let consumed = store
            .policy_admission_consume(
                &actor,
                &session.id,
                &decided.id,
                &"04".repeat(32),
                &evidence.authority_sha256(),
                Some(Strategy::Local),
                None,
            )
            .unwrap();
        assert_eq!(consumed.state, PolicyAdmissionState::Consumed);

        let intent = RemoteStartIntent {
            version: REMOTE_START_VERSION,
            original_invocation_id: "turn:1/call:remote-start".into(),
            turn_id: 1,
            call_id: "remote-start".into(),
            effect_attempt_id: "effect-1".into(),
            operation: "bash".into(),
            canonical_ref: canonical.into(),
            arguments_sha256: [9; 32],
            original_request_sha256: request_digest,
            session_action: RemoteSessionAction::Submit,
            target: RemoteRuntimeLease {
                target_host: "l5".into(),
                target_profile: "default".into(),
                generation: [7; 32],
                issued_at_unix_ms: 1_000,
                expires_at_unix_ms: 61_000,
            },
        };
        let first = authorize_remote_start(
            &store,
            &authority,
            &actor,
            &evidence,
            &session.id,
            &consumed.id,
            intent.clone(),
        )
        .unwrap();

        agent
            .revoke_registered_source_tool(&actor, "bash", &ToolSource::trusted_host())
            .unwrap();
        let replay = authorize_remote_start(
            &store,
            &authority,
            &actor,
            &evidence,
            &session.id,
            &consumed.id,
            intent.clone(),
        )
        .unwrap();
        assert_eq!(replay, first);

        let mut fresh = intent;
        fresh.effect_attempt_id = "effect-2".into();
        assert!(authorize_remote_start(
            &store,
            &authority,
            &actor,
            &evidence,
            &session.id,
            &consumed.id,
            fresh,
        )
        .is_err());
    }

    #[test]
    fn guardian_wire_is_strict_and_route_names_are_typed() {
        let decision: GuardianDecisionWire =
            serde_json::from_str(r#"{"decision":"ask","route":"local","approval_scope":"/scope"}"#)
                .unwrap();
        assert!(matches!(decision.decision, WireDecision::Ask));
        assert!(matches!(decision.route, Some(WireRoute::Local)));
        assert!(serde_json::from_str::<GuardianDecisionWire>(
            r#"{"decision":"allow","unexpected":true}"#
        )
        .is_err());
        assert!(serde_json::from_str::<GuardianDecisionWire>(
            r#"{"decision":"allow","route":"remote"}"#
        )
        .is_err());
    }
}
