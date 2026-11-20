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
    RemoteSessionAction, RemoteStartAuthorization, RemoteStartIntent, RemoteStartIssuer,
    RemoteStartProvenance, RemoteStartSource,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

const ACTION: &str = "remote.start.authorized";
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

fn attempt_key(actor: &PrincipalId, session: &str, intent: &RemoteStartIntent) -> Result<[u8; 32]> {
    intent
        .attempt_key(actor.as_str(), session)
        .map_err(|_| Error::Denied("invalid remote Start attempt identity"))
}

fn current_by_key(
    connection: &rusqlite::Connection,
    key: &[u8; 32],
) -> Result<Option<RemoteStartAuthorization>> {
    let encoded: Option<String> = db(connection
        .query_row(
            "SELECT authorization_json FROM remote_start_current WHERE attempt_key=?1",
            [key.as_slice()],
            |row| row.get(0),
        )
        .optional())?;
    encoded.map(|value| crate::decode(&value)).transpose()
}

fn store_current(
    connection: &rusqlite::Connection,
    authorization: &RemoteStartAuthorization,
) -> Result<()> {
    let actor = PrincipalId::new(authorization.provenance.actor.clone())?;
    let key = attempt_key(
        &actor,
        &authorization.provenance.session_id,
        &authorization.intent,
    )?;
    if let Some(existing) = current_by_key(connection, &key)? {
        return if existing == *authorization {
            Ok(())
        } else {
            Err(Error::Storage(
                "duplicate persisted remote Start attempt identity",
            ))
        };
    }
    let encoded = serde_json::to_string(authorization)
        .map_err(|_| Error::Storage("cannot serialize remote Start authorization"))?;
    db(connection.execute(
        "INSERT INTO remote_start_current(
            attempt_key,session_id,actor,original_invocation_id,effect_attempt_id,
            turn_id,authorization_json,created_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            key.as_slice(),
            authorization.provenance.session_id,
            authorization.provenance.actor,
            authorization.intent.original_invocation_id,
            authorization.intent.effect_attempt_id,
            authorization.intent.turn_id,
            encoded,
            crate::now(),
        ],
    ))?;
    Ok(())
}

pub(crate) fn backfill_current(connection: &rusqlite::Connection) -> Result<()> {
    let mut query =
        db(connection.prepare("SELECT data_json FROM audit WHERE action=?1 ORDER BY seq"))?;
    let rows = db(query.query_map([ACTION], |row| row.get::<_, String>(0)))?;
    for row in rows {
        let encoded = db(row)?;
        let authorization: RemoteStartAuthorization = crate::decode(&encoded)?;
        store_current(connection, &authorization)?;
    }
    Ok(())
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
    /// Recover an already-committed Start by the context-independent original
    /// effect-attempt identity, before any current ToolBinding reauthorization or
    /// contextual ref resolution. This does not make an expired proof executable;
    /// the remote owner still validates its target-issued lease and generation.
    pub fn remote_start_lookup_attempt(
        &self,
        actor: &PrincipalId,
        session: &str,
        original_invocation_id: &str,
        effect_attempt_id: &str,
    ) -> Result<Option<RemoteStartAuthorization>> {
        if original_invocation_id.is_empty()
            || original_invocation_id.len() > 512
            || !original_invocation_id.is_ascii()
            || original_invocation_id.chars().any(char::is_control)
            || effect_attempt_id.is_empty()
            || effect_attempt_id.len() > 256
            || !effect_attempt_id.is_ascii()
            || effect_attempt_id.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid remote Start replay identity"));
        }
        let connection = self.connection()?;
        let encoded: Option<String> = db(connection
            .query_row(
                "SELECT authorization_json FROM remote_start_current
                 WHERE session_id=?1 AND actor=?2
                   AND original_invocation_id=?3 AND effect_attempt_id=?4",
                params![
                    session,
                    actor.as_str(),
                    original_invocation_id,
                    effect_attempt_id
                ],
                |row| row.get(0),
            )
            .optional())?;
        encoded.map(|value| crate::decode(&value)).transpose()
    }

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
        let key = attempt_key(actor, session, intent)?;
        match current_by_key(&connection, &key)? {
            Some(existing) if existing.intent == *intent => Ok(Some(existing)),
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
        issuer: RemoteStartIssuer,
    ) -> Result<RemoteStartAuthorization> {
        intent
            .validate()
            .map_err(|_| Error::Denied("invalid remote Start intent"))?;
        source
            .validate()
            .map_err(|_| Error::Denied("invalid remote Start source provenance"))?;
        issuer
            .validate()
            .map_err(|_| Error::Denied("invalid or untrusted remote Start issuer identity"))?;

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let key = attempt_key(actor, session, &intent)?;
        if let Some(existing) = current_by_key(&tx, &key)? {
            if existing.intent != intent {
                return Err(Error::Conflict(
                    "remote Start attempt identity was reused with changed intent",
                ));
            }
            db(tx.commit())?;
            return Ok(existing);
        }

        let policy = policy_admission::fact_in(&tx, session, policy_admission_id)?;
        let resource_owner = crate::session_in(&tx, session)?.owner;
        if policy.id != policy_admission::admission_id(&resource_owner, &policy.request)
            || policy.request.subject.actor != *actor
        {
            return Err(Error::Denied(
                "remote Start policy admission belongs to another subject or Session owner",
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

        let unique: usize = db(tx.query_row(
            "SELECT count(*) FROM remote_start_current
             WHERE session_id=?1 AND actor=?2 AND turn_id=?3",
            params![session, actor.as_str(), intent.turn_id],
            |row| row.get(0),
        ))?;
        if unique >= MAX_STARTS_PER_TURN {
            return Err(Error::Limit(
                "remote Start attempts exceed the bounded active-Turn budget",
            ));
        }

        let provenance = RemoteStartProvenance {
            actor: actor.as_str().to_owned(),
            source,
            issuer: Some(issuer),
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
        store_current(&tx, &authorization)?;
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

    fn issuer() -> RemoteStartIssuer {
        RemoteStartIssuer {
            client_id: "aw-client".into(),
            trust_epoch: 1,
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
        let subject = policy_admission::PolicyAdmissionSubject::from_participation(
            &store
                .authorize_session_participation(
                    &actor,
                    &session.id,
                    session_participation::SessionParticipationAction::Submit,
                )
                .unwrap(),
        );
        let request = policy_admission::PolicyAdmissionRequest {
            session_id: session.id.clone(),
            branch_id: branch.id.clone(),
            writer_epoch,
            turn_id: 1,
            call_id: "remote-start".into(),
            canonical_target: "/d:local/i:owner/p:repo/a:0".into(),
            request_sha256: digest(2),
            subject,
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
        let claimed = store.policy_admission_claim(request).unwrap();
        let decided = store
            .policy_admission_record_decision(
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
            .remote_start_authorize(
                &actor,
                &session,
                &policy_id,
                intent.clone(),
                source(),
                issuer(),
            )
            .unwrap();

        let by_attempt = store
            .remote_start_lookup_attempt(
                &actor,
                &session,
                &intent.original_invocation_id,
                &intent.effect_attempt_id,
            )
            .unwrap()
            .unwrap();
        assert_eq!(by_attempt, first);

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
                .remote_start_authorize(
                    &actor,
                    &session,
                    &policy_id,
                    intent.clone(),
                    source(),
                    issuer()
                )
                .unwrap(),
            first
        );
        assert_eq!(
            store
                .remote_start_lookup_attempt(
                    &actor,
                    &session,
                    &intent.original_invocation_id,
                    &intent.effect_attempt_id,
                )
                .unwrap()
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
            .remote_start_authorize(&actor, &session, &policy_id, wrong, source(), issuer())
            .is_err());

        let valid = intent(request_digest);
        let authorization = store
            .remote_start_authorize(
                &actor,
                &session,
                &policy_id,
                valid.clone(),
                source(),
                issuer(),
            )
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
            .remote_start_authorize(
                &actor,
                &session,
                &policy_id,
                changed_attempt,
                wrong_source,
                issuer()
            )
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn v35_open_migrates_legacy_remote_start_and_agent_exposure_indexes() {
        let (temp, store, actor, session, policy_id, request_digest) = admitted_fixture().await;
        let intent = intent(request_digest);
        let current = store
            .remote_start_authorize(
                &actor,
                &session,
                &policy_id,
                intent.clone(),
                source(),
                issuer(),
            )
            .unwrap();

        let mut legacy_intent = intent;
        legacy_intent.effect_attempt_id = "attempt-legacy-v35".into();
        let mut legacy_provenance = current.provenance.clone();
        legacy_provenance.issuer = None;
        let legacy =
            RemoteStartAuthorization::new(legacy_intent.clone(), legacy_provenance).unwrap();
        assert!(legacy.provenance.issuer.is_none());
        {
            let connection = store.connection().unwrap();
            audit(
                &connection,
                &session,
                &actor,
                ACTION,
                serde_json::to_value(&legacy).unwrap(),
            )
            .unwrap();
            connection
                .execute_batch(
                    "DROP TABLE policy_admission_current;
                     DROP TABLE remote_start_current;
                     DROP TABLE agent_job_event_sources;
                     DROP TABLE agent_job_wake_admissions;
                     DROP TABLE job_exposure_current;
                     DROP INDEX sessions_agent_namespace;
                     DROP INDEX branches_agent_namespace;
                     UPDATE store_meta SET value='35' WHERE key='schema_epoch';
                     UPDATE schema_meta
                        SET schema_epoch=35,activation_epoch=35,migration_phase='ready',updated_at=0
                      WHERE singleton=1;
                     UPDATE migration_lease
                        SET holder_client_id=NULL,generation=generation+1,current_epoch=35,
                            target_epoch=NULL,lease_expires_at=NULL,updated_at=0
                      WHERE singleton=1;
                     PRAGMA user_version=35;",
                )
                .unwrap();
        }
        drop(store);

        let migrated = SessionStore::open(&temp.0).unwrap();
        let connection = migrated.connection().unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let projected_agents: u64 = connection
            .query_row(
                "SELECT count(*) FROM job_exposure_current WHERE kind='agent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 36);
        assert!(projected_agents >= 1);
        drop(connection);

        let replay = migrated
            .remote_start_lookup_attempt(
                &actor,
                &session,
                &legacy_intent.original_invocation_id,
                &legacy_intent.effect_attempt_id,
            )
            .unwrap()
            .unwrap();
        assert_eq!(replay, legacy);
        assert!(
            replay.provenance.issuer.is_none(),
            "v35 historical Start must remain exact rather than minting v36 issuer evidence"
        );
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
            .remote_start_authorize(
                &actor,
                &session,
                &policy_id,
                stale_writer,
                source(),
                issuer()
            )
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
            .remote_start_authorize(
                &actor,
                &session,
                &policy_id,
                revoked_member,
                source(),
                issuer()
            )
            .is_err());
    }
}
