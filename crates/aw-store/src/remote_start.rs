//! Durable one-shot AW -> remote Start authorization.
//!
//! The audit fact is the authorization linearization point. Exact replay is
//! looked up by the original context-independent attempt identity before any
//! current reauthorization; a changed retry can never mint a replacement Start.

use crate::{
    audit, branch_in, db, policy_admission, session_participation, workspace_writer, Error,
    PrincipalId, Result, SessionStore,
};
use aw_tools::remote_start::{
    RemoteSessionAction, RemoteStartAuthorization, RemoteStartIntent, RemoteStartProvenance,
    RemoteStartSource,
};
use rusqlite::{params, TransactionBehavior};
use std::collections::BTreeSet;

const ACTION: &str = "remote.start.authorized";
const MAX_EVENTS: usize = 8_192;
const MAX_STARTS_PER_TURN: usize = 256;

fn parse_digest(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Denied("invalid remote Start digest provenance"));
    }
    let mut out = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or(Error::Denied("invalid remote Start digest provenance"))?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or(Error::Denied("invalid remote Start digest provenance"))?;
        out[index] = ((high << 4) | low) as u8;
    }
    Ok(out)
}

fn events(
    connection: &rusqlite::Connection,
    session: &str,
) -> Result<Vec<RemoteStartAuthorization>> {
    let mut query = db(connection.prepare(
        "SELECT data_json FROM audit
         WHERE session_id=?1 AND action=?2
         ORDER BY seq LIMIT 8193",
    ))?;
    let rows = db(query.query_map(params![session, ACTION], |row| row.get::<_, String>(0)))?;
    let encoded = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if encoded.len() > MAX_EVENTS {
        return Err(Error::Limit(
            "remote Start audit requires explicit inspection/rotation",
        ));
    }
    encoded
        .into_iter()
        .map(|value| crate::decode::<RemoteStartAuthorization>(&value))
        .collect()
}

fn existing_attempt<'a>(
    facts: &'a [RemoteStartAuthorization],
    actor: &PrincipalId,
    session: &str,
    intent: &RemoteStartIntent,
) -> Result<Option<&'a RemoteStartAuthorization>> {
    let key = intent
        .attempt_key(actor.as_str(), session)
        .map_err(|_| Error::Denied("invalid remote Start attempt identity"))?;
    for fact in facts.iter().rev() {
        let fact_key = fact
            .intent
            .attempt_key(&fact.provenance.actor, &fact.provenance.session_id)
            .map_err(|_| Error::Storage("invalid persisted remote Start fact"))?;
        if fact_key == key {
            return Ok(Some(fact));
        }
    }
    Ok(None)
}

fn require_running_turn(
    connection: &rusqlite::Connection,
    session: &str,
    branch: &str,
    turn_id: u64,
) -> Result<()> {
    let branch_row = branch_in(connection, session, branch)?;
    if branch_row.id != branch {
        return Err(Error::Denied(
            "remote Start requires the exact canonical branch",
        ));
    }
    let run = branch_row.active_run.as_deref().ok_or(Error::Conflict(
        "remote Start requires an active original Turn",
    ))?;
    let current: u64 = db(connection.query_row(
        "SELECT turn_id FROM runs
         WHERE id=?1 AND session_id=?2 AND branch_id=?3 AND status='running'",
        params![run, session, branch],
        |row| row.get(0),
    ))?;
    if current != turn_id {
        return Err(Error::Conflict(
            "remote Start targets a different active Turn",
        ));
    }
    Ok(())
}

fn session_action(
    action: RemoteSessionAction,
) -> session_participation::SessionParticipationAction {
    match action {
        RemoteSessionAction::Submit => session_participation::SessionParticipationAction::Submit,
        RemoteSessionAction::Control => session_participation::SessionParticipationAction::Control,
    }
}

impl SessionStore {
    /// Recover an already-committed Start without current effect reauthorization.
    /// This does not make an expired proof executable; the remote owner still
    /// validates its target-issued lease and live runtime generation.
    pub fn remote_start_lookup(
        &self,
        actor: &PrincipalId,
        session: &str,
        intent: &RemoteStartIntent,
    ) -> Result<Option<RemoteStartAuthorization>> {
        intent
            .validate()
            .map_err(|_| Error::Denied("invalid remote Start intent"))?;
        let connection = self.connection()?;
        let facts = events(&connection, session)?;
        match existing_attempt(&facts, actor, session, intent)? {
            Some(existing) if existing.intent == *intent => Ok(Some(existing.clone())),
            Some(_) => Err(Error::Conflict(
                "remote Start attempt identity was reused with changed intent",
            )),
            None => Ok(None),
        }
    }

    /// Atomically recheck Store-owned participation/writer/Turn/policy evidence
    /// and commit one immutable Start authorization. The caller must serialize
    /// this call under the exact live ToolBinding read guard so source/site/grant
    /// revocation cannot cross the commit boundary.
    pub fn remote_start_authorize(
        &self,
        actor: &PrincipalId,
        session: &str,
        policy_admission_id: &str,
        intent: RemoteStartIntent,
        source: RemoteStartSource,
    ) -> Result<RemoteStartAuthorization> {
        intent
            .validate()
            .map_err(|_| Error::Denied("invalid remote Start intent"))?;
        source
            .validate()
            .map_err(|_| Error::Denied("invalid remote Start source provenance"))?;

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let facts = events(&tx, session)?;
        if let Some(existing) = existing_attempt(&facts, actor, session, &intent)? {
            if existing.intent != intent {
                return Err(Error::Conflict(
                    "remote Start attempt identity was reused with changed intent",
                ));
            }
            db(tx.commit())?;
            return Ok(existing.clone());
        }

        let policy = policy_admission::fact_in(&tx, session, policy_admission_id)?;
        if policy.id != policy_admission::admission_id(actor, &policy.request) {
            return Err(Error::Denied(
                "remote Start policy admission belongs to another actor",
            ));
        }
        if policy.state != policy_admission::PolicyAdmissionState::Consumed {
            return Err(Error::Conflict(
                "remote Start requires one consumed executable policy admission",
            ));
        }
        if policy.request.session_id != session
            || policy.request.branch_id.is_empty()
            || policy.request.turn_id != intent.turn_id
            || policy.request.call_id != intent.call_id
            || policy.request.canonical_target != intent.canonical_ref
            || parse_digest(&policy.request.request_sha256)? != intent.original_request_sha256
            || parse_digest(&policy.request.authority_sha256)? != source.grant_sha256
            || source.resource_scope != policy.request.canonical_target
        {
            return Err(Error::Denied(
                "remote Start differs from the admitted original semantic request",
            ));
        }
        let expected_invocation = format!("turn:{}/call:{}", intent.turn_id, intent.call_id);
        if intent.original_invocation_id != expected_invocation {
            return Err(Error::Denied(
                "remote Start invocation identity differs from the original ToolCall",
            ));
        }

        require_running_turn(
            &tx,
            session,
            &policy.request.branch_id,
            policy.request.turn_id,
        )?;
        let writer = workspace_writer::verify_writer_epoch_for_session(
            &tx,
            session,
            policy.request.writer_epoch,
        )?;
        if writer.state != "active" {
            return Err(Error::Denied(
                "remote Start writer epoch is no longer active",
            ));
        }
        let participant = session_participation::authorize_in(
            &tx,
            actor,
            session,
            session_action(intent.session_action),
        )?;

        let mut unique = BTreeSet::new();
        for fact in &facts {
            if fact.provenance.actor == actor.as_str()
                && fact.provenance.session_id == session
                && fact.intent.turn_id == intent.turn_id
            {
                unique.insert(fact.intent.effect_attempt_id.as_str());
            }
        }
        if unique.len() >= MAX_STARTS_PER_TURN {
            return Err(Error::Limit(
                "remote Start attempts exceed the bounded active-Turn budget",
            ));
        }

        let provenance = RemoteStartProvenance {
            actor: actor.as_str().to_owned(),
            source,
            policy_revision_sha256: parse_digest(&policy.request.policy_revision_sha256)?,
            policy_admission_id: policy.id,
            session_id: session.to_owned(),
            branch_id: policy.request.branch_id.clone(),
            writer_epoch: policy.request.writer_epoch,
            membership_epoch: participant.membership_epoch,
            audience_epoch: participant.audience_epoch,
        };
        let authorization = RemoteStartAuthorization::new(intent, provenance)
            .map_err(|_| Error::Denied("invalid committed remote Start authorization"))?;
        audit(
            &tx,
            session,
            actor,
            ACTION,
            serde_json::to_value(&authorization)
                .map_err(|_| Error::Storage("cannot serialize remote Start authorization"))?,
        )?;
        db(tx.commit())?;
        Ok(authorization)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, PrincipalId, Provider,
        TurnJournal, TurnStart, Usage,
    };
    use aw_policy::{Decision, PolicyDecision, Strategy};
    use aw_tools::remote_start::{
        RemoteRuntimeLease, RemoteStartIntent, RemoteStartSource, REMOTE_START_VERSION,
    };
    use std::num::NonZeroU64;

    #[cfg(unix)]
    struct EchoProvider;

    #[cfg(unix)]
    impl Provider for EchoProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [aw_agent_core::Message],
            _model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                sink("ok")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    usage: Usage::default(),
                })
            })
        }
    }

    #[cfg(unix)]
    struct Temp(std::path::PathBuf);

    #[cfg(unix)]
    impl Temp {
        fn new() -> Self {
            use std::os::unix::fs::DirBuilderExt;
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "aw-remote-start-{}-{}",
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
    fn binding() -> aw_store_api::SessionBinding {
        aw_store_api::SessionBinding {
            working_directory: std::path::PathBuf::from("/tmp"),
            tool_root: std::path::PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[cfg(unix)]
    fn digest(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    #[cfg(unix)]
    fn source() -> RemoteStartSource {
        RemoteStartSource {
            source_kind: "host".into(),
            source_id: "trusted-host".into(),
            source_epoch: 1,
            site: "dotlink:l5".into(),
            resource_scope: "/d:local/i:owner/p:repo/a:0".into(),
            grant_sha256: [3; 32],
        }
    }

    #[cfg(unix)]
    fn intent(request_digest: [u8; 32]) -> RemoteStartIntent {
        RemoteStartIntent {
            version: REMOTE_START_VERSION,
            original_invocation_id: "turn:1/call:remote-start".into(),
            turn_id: 1,
            call_id: "remote-start".into(),
            effect_attempt_id: "attempt-1".into(),
            operation: "bash".into(),
            canonical_ref: "/d:local/i:owner/p:repo/a:0".into(),
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
        }
    }

    #[cfg(unix)]
    async fn admitted_fixture() -> (Temp, SessionStore, PrincipalId, String, String, [u8; 32]) {
        let temp = Temp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let mut agent = Agent::builder(
            std::sync::Arc::new(EchoProvider),
            AgentOptions::new("fixture", "model"),
        )
        .build()
        .unwrap();
        let actor = agent.actor().clone();
        let (session, branch) = store.create(&actor, "remote", binding()).unwrap();
        let (view, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        agent
            .load_history(view.history, Some(std::sync::Arc::new(handle.clone())))
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
        let writer_epoch = store
            .workspace_writer(&actor, None, &session.workspace_id)
            .unwrap()
            .writer_epoch;
        assert!(NonZeroU64::new(writer_epoch).is_some());

        let request_digest = [2; 32];
        let request = policy_admission::PolicyAdmissionRequest {
            session_id: session.id.clone(),
            branch_id: branch.id.clone(),
            writer_epoch,
            turn_id: 1,
            call_id: "remote-start".into(),
            canonical_target: "/d:local/i:owner/p:repo/a:0".into(),
            request_sha256: digest(2),
            policy_revision_sha256: digest(4),
            authority_sha256: digest(3),
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
                &digest(8),
            )
            .unwrap();
        let consumed = store
            .policy_admission_consume(
                &actor,
                &session.id,
                &decided.id,
                &digest(4),
                &digest(3),
                Some(Strategy::Local),
                None,
            )
            .unwrap();
        assert_eq!(
            consumed.state,
            policy_admission::PolicyAdmissionState::Consumed
        );
        (temp, store, actor, session.id, consumed.id, request_digest)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exact_start_replay_precedes_current_reauthorization_and_never_retargets() {
        let (_temp, store, actor, session, policy_id, request_digest) = admitted_fixture().await;
        let intent = intent(request_digest);
        let first = store
            .remote_start_authorize(&actor, &session, &policy_id, intent.clone(), source())
            .unwrap();

        let replay = store
            .remote_start_lookup(&actor, &session, &intent)
            .unwrap()
            .unwrap();
        assert_eq!(replay, first);

        // Revocation after the authorization linearization point cannot turn
        // an exact redelivery into a new Start decision.
        {
            let connection = store.connection().unwrap();
            let user = crate::user_for_principal(&connection, &actor)
                .unwrap()
                .unwrap();
            connection
                .execute(
                    "UPDATE users SET status='suspended',security_epoch=security_epoch+1 WHERE user_id=?1",
                    [user],
                )
                .unwrap();
        }
        assert_eq!(
            store
                .remote_start_authorize(&actor, &session, &policy_id, intent.clone(), source(),)
                .unwrap(),
            first
        );

        let mut changed = intent.clone();
        changed.target.generation = [8; 32];
        assert!(matches!(
            store.remote_start_lookup(&actor, &session, &changed),
            Err(Error::Conflict(_))
        ));

        assert_eq!(
            store
                .remote_start_lookup(&actor, &session, &intent)
                .unwrap()
                .unwrap(),
            first
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_commit_rechecks_writer_participation_policy_and_original_request() {
        let (_temp, store, actor, session, policy_id, request_digest) = admitted_fixture().await;

        let mut wrong = intent(request_digest);
        wrong.original_request_sha256 = [5; 32];
        assert!(store
            .remote_start_authorize(&actor, &session, &policy_id, wrong, source())
            .is_err());

        let valid = intent(request_digest);
        let authorization = store
            .remote_start_authorize(&actor, &session, &policy_id, valid.clone(), source())
            .unwrap();
        assert_eq!(authorization.provenance.actor, actor.as_str());
        assert!(authorization.provenance.writer_epoch > 0);
        assert!(authorization.provenance.membership_epoch > 0);
        assert!(authorization.provenance.audience_epoch > 0);

        let mut changed_attempt = valid;
        changed_attempt.effect_attempt_id = "attempt-2".into();
        let mut wrong_source = source();
        wrong_source.grant_sha256 = [6; 32];
        assert!(store
            .remote_start_authorize(&actor, &session, &policy_id, changed_attempt, wrong_source)
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn writer_or_membership_revoke_before_start_denies_new_attempt() {
        let (_temp, store, actor, session, policy_id, request_digest) = admitted_fixture().await;
        let session_info = store.session(&actor, &session).unwrap();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "UPDATE workspace_writer_state SET writer_epoch=writer_epoch+1,revision=revision+1 WHERE workspace_id=?1",
                    [&session_info.workspace_id],
                )
                .unwrap();
        }
        let mut stale_writer = intent(request_digest);
        stale_writer.effect_attempt_id = "attempt-writer-revoked".into();
        assert!(store
            .remote_start_authorize(&actor, &session, &policy_id, stale_writer, source())
            .is_err());

        let (_temp, store, actor, session, policy_id, request_digest) = admitted_fixture().await;
        {
            let connection = store.connection().unwrap();
            let user = crate::user_for_principal(&connection, &actor)
                .unwrap()
                .unwrap();
            connection
                .execute(
                    "UPDATE users SET status='suspended',security_epoch=security_epoch+1 WHERE user_id=?1",
                    [user],
                )
                .unwrap();
        }
        let mut revoked_member = intent(request_digest);
        revoked_member.effect_attempt_id = "attempt-member-revoked".into();
        assert!(store
            .remote_start_authorize(&actor, &session, &policy_id, revoked_member, source())
            .is_err());
    }
}
