use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::de::DeserializeOwned;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

const MAX_COLLAB_BODY: usize = aw_collab::MAX_UPDATE_BYTES * 2 + 16 * 1024;
const PRESENCE_TTL: Duration = Duration::from_secs(30);
const MAX_PRESENCE_RECORDS: usize = 128;
const MAX_PRESENCE_PER_PARTICIPANT: usize = 8;

#[derive(Default)]
pub(super) struct State {
    pub(super) presence: Mutex<BTreeMap<(String, u64, String, String), PresenceRecord>>,
    /// Admission lifetime for optimistic presence writes. Share/mode changes
    /// advance this under the same presence lock used for publication.
    presence_epoch: AtomicU64,
    #[cfg(test)]
    pub(super) held_presence_post: Mutex<Option<HeldPresencePost>>,
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct HeldPresencePost {
    pub(super) client_id: String,
    pub(super) reached: Arc<tokio::sync::Semaphore>,
    pub(super) release: Arc<tokio::sync::Semaphore>,
}

#[derive(Clone)]
pub(super) struct PresenceRecord {
    session_id: String,
    participant_id: String,
    display_name: String,
    cursor_anchor: u32,
    cursor_head: u32,
    pub(super) updated_at: Instant,
}

#[cfg(test)]
impl PresenceRecord {
    pub(super) fn for_test(session_id: &str, participant: &str) -> Self {
        Self {
            session_id: session_id.to_owned(),
            participant_id: participant.to_owned(),
            display_name: participant.to_owned(),
            cursor_anchor: 0,
            cursor_head: 0,
            updated_at: Instant::now(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShareCommand {
    participant_id: String,
    enabled: bool,
    #[serde(default)]
    mode: Option<aw_collab::DraftMode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeCommand {
    mode: aw_collab::DraftMode,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenSharedCommand {
    expected_revision: u64,
    session_id: String,
    branch: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftOpenCommand {
    #[serde(default)]
    activate_auto: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftRefCommand {
    draft_id: String,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftUpdateCommand {
    draft_id: String,
    generation: u64,
    document_epoch: u64,
    update_id: String,
    update_v1_b64: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftSubmitCommand {
    expected_revision: u64,
    draft_id: String,
    generation: u64,
    document_epoch: u64,
    accepted_revision: u64,
    metadata_revision: u64,
    digest_sha256: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftSubmissionCommand {
    submission_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresenceCommand {
    draft_id: String,
    generation: u64,
    client_id: String,
    cursor_anchor: u32,
    cursor_head: u32,
    active: bool,
}

pub(super) fn is_route(path: &str, method: &Method) -> bool {
    matches!(
        (path, method),
        ("/api/team/collaborators", &Method::GET)
            | ("/api/team/shared-sessions", &Method::GET)
            | ("/api/team/session/collaboration", &Method::GET)
            | ("/api/team/session/mode", &Method::POST)
            | ("/api/team/session/share", &Method::POST)
            | ("/api/team/session/open-shared", &Method::POST)
            | ("/api/team/draft/open", &Method::POST)
            | ("/api/team/draft/status", &Method::POST)
            | ("/api/team/draft/update", &Method::POST)
            | ("/api/team/draft/submit", &Method::POST)
            | ("/api/team/draft/submission", &Method::POST)
            | ("/api/team/draft/recover", &Method::POST)
            | ("/api/team/draft/presence", &Method::POST)
            | ("/api/team/draft/presence/list", &Method::POST)
    )
}

async fn body_json<T: DeserializeOwned>(request: Request) -> ApiResult<T> {
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), MAX_COLLAB_BODY),
    )
    .await
    .map_err(|_| ApiError(StatusCode::REQUEST_TIMEOUT, "collaboration body timeout"))?
    .map_err(|_| {
        ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "collaboration body too large",
        )
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid collaboration request"))
}

fn current(entry: &Entry) -> ApiResult<PersistentSession> {
    current_persistence(&entry.app)
}

fn snapshot_json(snapshot: &aw_store::DraftSnapshot) -> serde_json::Value {
    serde_json::json!({
        "draft_id": snapshot.draft_id,
        "generation": snapshot.generation,
        "document_epoch": snapshot.document_epoch,
        "accepted_revision": snapshot.accepted_revision,
        "metadata_revision": snapshot.metadata_revision,
        "state": snapshot.state,
        "mode": snapshot.mode,
        "target": snapshot.target,
        "text": snapshot.text,
        "update_v1_b64": BASE64.encode(&snapshot.update_v1),
        "candidate_sha256": aw_collab::candidate_digest(
            &snapshot.text,
            &snapshot.target,
            snapshot.metadata_revision,
        ),
    })
}

fn candidate_json(candidate: &aw_store::SealedCandidate) -> serde_json::Value {
    serde_json::json!({
        "submission_id": candidate.submission_id,
        "draft_id": candidate.draft_id,
        "generation": candidate.generation,
        "accepted_revision": candidate.accepted_revision,
        "metadata_revision": candidate.metadata_revision,
        "target": candidate.target,
        "text": candidate.text,
        "candidate_sha256": candidate.digest_sha256,
        "submitted_by_user_id": candidate.submitted_by_user_id,
        "contributor_user_ids": candidate.contributor_user_ids,
        "status": candidate.status,
        "prompt_request_id": candidate.prompt_request_id,
    })
}

fn entry_for_participant<'a>(team: &'a TeamState, id: &str) -> ApiResult<&'a Entry> {
    team.0
        .entries
        .iter()
        .find(|candidate| candidate.user.id == id)
        .ok_or(ApiError(
            StatusCode::BAD_REQUEST,
            "unknown collaboration participant",
        ))
}

fn emit(entry: &Entry, kind: &'static str, data: serde_json::Value) {
    let mut state = lock(&entry.app.0.session.state);
    state.snapshot.sequence = state.snapshot.sequence.saturating_add(1);
    let sequence = state.snapshot.sequence;
    let _ = entry.app.0.session.events.send(WireEvent {
        sequence,
        kind,
        data,
    });
}

fn broadcast(
    team: &TeamState,
    session_id: &str,
    kind: &'static str,
    data: serde_json::Value,
    presence_only: bool,
) {
    for entry in &team.0.entries {
        let Some(persistence) = lock(&entry.app.0.session.persistence).clone() else {
            continue;
        };
        let Ok(grant) = persistence
            .store
            .draft_grant(&entry.app.0.session.actor, session_id)
        else {
            continue;
        };
        if grant.grant.can_read && (!presence_only || grant.grant.presence_read) {
            emit(entry, kind, data.clone());
        }
    }
}

fn cleanup_presence(state: &mut BTreeMap<(String, u64, String, String), PresenceRecord>) {
    let now = Instant::now();
    state.retain(|_, value| now.duration_since(value.updated_at) <= PRESENCE_TTL);
}

/// Every successful mode or audience revision abandons the old shared-draft
/// generation. Withdraw *all* cursors of that Session before responding, even
/// if the same participant retains a grant. Older in-flight POSTs carry the
/// previous epoch and cannot republish that generation.
pub(super) fn invalidate_presence(team: &TeamState, session_id: &str) {
    let mut presence = lock(&team.0.multiplayer.presence);
    let _ = team.0.multiplayer.presence_epoch.fetch_update(
        AtomicOrdering::AcqRel,
        AtomicOrdering::Acquire,
        |epoch| epoch.checked_add(1),
    );
    // Exhaustion permanently disarms writes; it cannot revive an old epoch.
    presence.retain(|_, value| value.session_id != session_id);
}

/// Test-only synthetic audience at the hard capacity limit. Production callers
/// cannot bypass the per-participant and total-record admission checks.
#[cfg(all(test, feature = "oidc"))]
pub(super) fn seed_bounded_presence_for_http_measurement(
    state: &mut BTreeMap<(String, u64, String, String), PresenceRecord>,
    draft: &str,
    generation: u64,
    session_id: &str,
    count: usize,
) {
    assert!(count <= MAX_PRESENCE_RECORDS);
    state.clear();
    for index in 0..count {
        let (participant, client) = if index == 0 {
            ("bob".to_owned(), "bob-tab".to_owned())
        } else {
            (
                format!("synthetic-member-{index}"),
                format!("synthetic-client-{index}"),
            )
        };
        state.insert(
            (draft.to_owned(), generation, participant.clone(), client),
            PresenceRecord {
                session_id: session_id.to_owned(),
                participant_id: participant,
                display_name: "synthetic, not a real participant".to_owned(),
                cursor_anchor: 0,
                cursor_head: 0,
                updated_at: Instant::now(),
            },
        );
    }
}

fn validate_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

fn shared_current(
    entry: &Entry,
) -> ApiResult<Option<(PersistentSession, aw_store::DraftGrantInfo)>> {
    let persistence = current(entry)?;
    let session = persistence
        .store
        .session(&persistence.actor, persistence.handle.session_id())
        .map_err(api_error)?;
    if session.owner == persistence.actor {
        return Ok(None);
    }
    match persistence
        .store
        .draft_grant(&persistence.actor, &session.id)
    {
        Ok(grant) if grant.grant.can_read => Ok(Some((persistence, grant))),
        _ => Ok(None),
    }
}

pub(super) fn guard_forward(entry: &Entry, path: &str, method: &Method) -> ApiResult<()> {
    if method != Method::POST || shared_current(entry)?.is_none() {
        return Ok(());
    }
    let blocked = matches!(
        path,
        "/api/prompts" | "/api/fork" | "/api/checkpoint" | "/api/reset"
    ) || path.starts_with("/api/context/");
    if blocked {
        Err(ApiError(
            StatusCode::FORBIDDEN,
            "peer-owned shared Session mutations must use the exact shared-draft submit path",
        ))
    } else {
        Ok(())
    }
}

pub(super) async fn route(
    team: &TeamState,
    entry: &Entry,
    path: &str,
    request: Request,
) -> ApiResult<Response> {
    match path {
        "/api/team/collaborators" => {
            let mut rows = Vec::new();
            for candidate in team
                .0
                .entries
                .iter()
                .filter(|candidate| candidate.user.id != entry.user.id)
            {
                let persistence = current(candidate)?;
                let actor = persistence.actor.clone();
                let user_id =
                    store_work(move || persistence.store.collaboration_user_id(&actor)).await?;
                rows.push(serde_json::json!({
                    "id": candidate.user.id,
                    "user_id": user_id,
                    "display_name": candidate.user.display_name,
                }));
            }
            Ok(Json(serde_json::json!({"participants": rows})).into_response())
        }
        "/api/team/session/collaboration" => {
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let session_id = persistence.handle.session_id().to_owned();
            let (policy, grants) = store_work(move || {
                let info = persistence.store.session(&actor, &session_id)?;
                if info.owner != actor {
                    let grant = persistence.store.draft_grant(&actor, &session_id)?;
                    let policy = persistence.store.draft_policy(&actor, &session_id)?;
                    return Ok((policy, vec![grant]));
                }
                Ok((
                    persistence.store.draft_policy(&actor, &session_id)?,
                    persistence.store.draft_grants(&actor, &session_id)?,
                ))
            })
            .await?;
            Ok(Json(serde_json::json!({"policy":policy,"grants":grants})).into_response())
        }
        "/api/team/shared-sessions" => {
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let rows = store_work(move || {
                persistence
                    .store
                    .draft_shared_sessions(&actor)?
                    .into_iter()
                    .filter_map(|session| {
                        let grant = persistence.store.draft_grant(&actor, &session.id).ok()?;
                        let policy = persistence.store.draft_policy(&actor, &session.id).ok()?;
                        let branches = persistence.store.branches(&actor, &session.id).ok()?;
                        Some(serde_json::json!({
                            "id": session.id,
                            "title": session.title,
                            "updated_at": session.updated_at,
                            "directory": session.binding.working_directory,
                            "mode": policy.mode,
                            "grant": grant.grant,
                            "branches": branches,
                        }))
                    })
                    .collect::<Vec<_>>()
                    .pipe(Ok)
            })
            .await?;
            Ok(Json(serde_json::json!({"sessions": rows})).into_response())
        }
        "/api/team/session/mode" => {
            let command: ModeCommand = body_json(request).await?;
            let persistence = current(entry)?;
            let session_id = persistence.handle.session_id().to_owned();
            let actor = persistence.actor.clone();
            let policy = store_work(move || {
                persistence
                    .store
                    .set_draft_mode(&actor, &session_id, command.mode)
            })
            .await?;
            // Mode transitions retire all old cursor proposals; readers only
            // reappear after a fresh authorized POST on the new policy.
            invalidate_presence(team, &policy.session_id);
            broadcast(
                team,
                &policy.session_id,
                "draft_scope_changed",
                serde_json::json!({"mode":policy.mode,"revision":policy.revision}),
                false,
            );
            Ok(Json(serde_json::json!({"policy":policy})).into_response())
        }
        "/api/team/session/share" => {
            let command: ShareCommand = body_json(request).await?;
            if command.participant_id == entry.user.id {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "a participant cannot share a Session with itself",
                ));
            }
            let target = entry_for_participant(team, &command.participant_id)?.clone();
            let persistence = current(entry)?;
            let session_id = persistence.handle.session_id().to_owned();
            let invalidation_session_id = session_id.clone();
            let owner = persistence.actor.clone();
            let target_actor = target.app.0.session.actor.clone();
            let enabled = command.enabled;
            let mode = command.mode;
            let (policy, grant) = store_work(move || {
                let info = persistence.store.session(&owner, &session_id)?;
                if info.owner != owner {
                    return Err(Error::Denied(
                        "only the Session owner may change collaboration sharing",
                    ));
                }
                if let Some(mode) = mode {
                    persistence
                        .store
                        .set_draft_mode(&owner, &session_id, mode)?;
                }
                let policy = persistence.store.draft_policy(&owner, &session_id)?;
                if enabled && policy.mode == aw_collab::DraftMode::Off {
                    return Err(Error::Denied(
                        "enable Auto or On before sharing this Session",
                    ));
                }
                let grant = persistence.store.set_draft_grant(
                    &owner,
                    &session_id,
                    &target_actor,
                    enabled.then_some(aw_collab::DraftGrant::editor_submitter()),
                )?;
                Ok((policy, grant))
            })
            .await?;
            // set_draft_grant rotates the audience revision and abandons its
            // active CRDT generation even if the textual grant remains similar.
            invalidate_presence(team, &invalidation_session_id);
            broadcast(
                team,
                current(entry)?.handle.session_id(),
                "draft_scope_changed",
                serde_json::json!({"participant_id": command.participant_id}),
                false,
            );
            Ok(Json(serde_json::json!({"policy": policy, "grant": grant})).into_response())
        }
        "/api/team/session/open-shared" => {
            let command: OpenSharedCommand = body_json(request).await?;
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let session_id = command.session_id.clone();
            let allowed = store_work(move || {
                let grant = persistence.store.draft_grant(&actor, &session_id)?;
                if !grant.grant.can_read {
                    return Err(Error::Denied("shared Session read permission required"));
                }
                Ok(())
            })
            .await;
            if allowed.is_err() {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "shared Session unavailable or not permitted",
                ));
            }
            let Json(snapshot) = change_view_with_membership(
                &entry.app,
                command.expected_revision,
                ViewChange::Open(command.session_id, command.branch),
                true,
            )
            .await?;
            Ok(Json(snapshot).into_response())
        }
        "/api/team/draft/open" => {
            let command: DraftOpenCommand = body_json(request).await?;
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let session = persistence.handle.session_id().to_owned();
            let branch = persistence.handle.branch().map_err(api_error)?.id;
            let snapshot = store_work(move || {
                persistence.store.open_shared_draft(
                    &actor,
                    &session,
                    &branch,
                    command.activate_auto,
                )
            })
            .await?;
            Ok(Json(snapshot_json(&snapshot)).into_response())
        }
        "/api/team/draft/status" => {
            let command: DraftRefCommand = body_json(request).await?;
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let snapshot = store_work(move || {
                persistence.store.shared_draft_snapshot(
                    &actor,
                    &command.draft_id,
                    command.generation,
                )
            })
            .await?;
            Ok(Json(snapshot_json(&snapshot)).into_response())
        }
        "/api/team/draft/update" => {
            let command: DraftUpdateCommand = body_json(request).await?;
            let update = BASE64
                .decode(command.update_v1_b64.as_bytes())
                .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid shared-draft update"))?;
            if update.len() > aw_collab::MAX_UPDATE_BYTES {
                return Err(ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "shared-draft update too large",
                ));
            }
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let draft_id = command.draft_id.clone();
            let update_id = command.update_id.clone();
            let update_for_store = update.clone();
            let snapshot = store_work(move || {
                persistence.store.apply_shared_draft_update(
                    &actor,
                    &command.draft_id,
                    command.generation,
                    command.document_epoch,
                    &command.update_id,
                    &update_for_store,
                )
            })
            .await?;
            broadcast(
                team,
                &snapshot.target.session_id,
                "draft_changed",
                serde_json::json!({
                    "draft_id": draft_id,
                    "generation": snapshot.generation,
                    "accepted_revision": snapshot.accepted_revision,
                    "update_id": update_id,
                    "update_v1_b64": BASE64.encode(&update),
                    "participant": {
                        "id": entry.user.id,
                        "display_name": entry.user.display_name,
                    }
                }),
                false,
            );
            Ok(Json(snapshot_json(&snapshot)).into_response())
        }
        "/api/team/draft/submit" => {
            let command: DraftSubmitCommand = body_json(request).await?;
            if command.account_id.is_some() != command.model.is_some() {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "select both account and model, or neither",
                ));
            }
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let sealed = store_work(move || {
                persistence.store.seal_shared_draft(
                    &actor,
                    &command.draft_id,
                    command.generation,
                    command.document_epoch,
                    command.accepted_revision,
                    command.metadata_revision,
                    &command.digest_sha256,
                )
            })
            .await?;
            let request_id = format!("draft_{}", sealed.submission_id);
            let first_execution = sealed.prompt_request_id.is_none();
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let submission_id = sealed.submission_id.clone();
            let request_id_for_store = request_id.clone();
            let bound = store_work(move || {
                persistence.store.bind_shared_draft_prompt_request(
                    &actor,
                    &submission_id,
                    &request_id_for_store,
                )
            })
            .await?;
            let target = bound.target.clone();
            let mut settle_events = first_execution.then(|| entry.app.0.session.events.subscribe());
            let prompt_command = PromptCommand {
                request_id,
                expected_revision: command.expected_revision,
                prompt: bound.text.clone(),
                account_id: command.account_id,
                model: command.model,
                session_id: Some(target.session_id.clone()),
                branch_id: Some(target.branch_id.clone()),
                retry_operation_id: None,
            };
            let (status, Json(receipt)) = prompt(
                axum::extract::State(entry.app.clone()),
                Ok(Json(prompt_command)),
            )
            .await?;
            // HTTP acceptance only reserves the exact request. The journal
            // begin transaction marks the shared candidate admitted when it
            // actually creates the prompt node/run and advances the branch.
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let submission_id = bound.submission_id.clone();
            let candidate = store_work(move || {
                persistence
                    .store
                    .shared_draft_submission(&actor, &submission_id)
            })
            .await?;
            broadcast(
                team,
                &target.session_id,
                "draft_submitted",
                serde_json::json!({
                    "submission_id": candidate.submission_id,
                    "generation": candidate.generation,
                    "participant": {
                        "id": entry.user.id,
                        "display_name": entry.user.display_name,
                    }
                }),
                false,
            );
            if let Some(mut events) = settle_events.take() {
                let watcher_team = team.clone();
                let watcher_entry = entry.clone();
                let watcher_session = target.session_id.clone();
                let watcher_submission = candidate.submission_id.clone();
                tokio::spawn(async move {
                    let deadline = tokio::time::sleep(Duration::from_secs(30 * 60));
                    tokio::pin!(deadline);
                    loop {
                        if lock(&watcher_entry.app.0.session.state)
                            .snapshot
                            .pending
                            .is_none()
                        {
                            broadcast(
                                &watcher_team,
                                &watcher_session,
                                "draft_settled",
                                serde_json::json!({"submission_id":watcher_submission}),
                                false,
                            );
                            break;
                        }
                        tokio::select! {
                            _ = watcher_team.0.stop.cancelled() => break,
                            _ = &mut deadline => break,
                            event = events.recv() => {
                                if event.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
            Ok((
                status,
                Json(serde_json::json!({
                    "submission": candidate_json(&candidate),
                    "receipt": receipt,
                })),
            )
                .into_response())
        }
        "/api/team/draft/submission" => {
            let command: DraftSubmissionCommand = body_json(request).await?;
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let candidate = store_work(move || {
                persistence
                    .store
                    .shared_draft_submission(&actor, &command.submission_id)
            })
            .await?;
            Ok(Json(candidate_json(&candidate)).into_response())
        }
        "/api/team/draft/recover" => {
            let command: DraftSubmissionCommand = body_json(request).await?;
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let snapshot = store_work(move || {
                persistence
                    .store
                    .recover_shared_draft(&actor, &command.submission_id)
            })
            .await?;
            broadcast(
                team,
                &snapshot.target.session_id,
                "draft_changed",
                serde_json::json!({
                    "draft_id": snapshot.draft_id,
                    "generation": snapshot.generation,
                    "accepted_revision": snapshot.accepted_revision,
                    "recovered": true,
                }),
                false,
            );
            Ok(Json(snapshot_json(&snapshot)).into_response())
        }
        "/api/team/draft/presence" => {
            let command: PresenceCommand = body_json(request).await?;
            let admission_epoch = team
                .0
                .multiplayer
                .presence_epoch
                .load(AtomicOrdering::Acquire);
            if !validate_client_id(&command.client_id) {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "invalid presence client ID",
                ));
            }
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let presence_draft_id = command.draft_id.clone();
            let presence_generation = command.generation;
            let snapshot = store_work(move || {
                let snapshot = persistence.store.shared_draft_snapshot(
                    &actor,
                    &command.draft_id,
                    command.generation,
                )?;
                let grant = persistence
                    .store
                    .draft_grant(&actor, &snapshot.target.session_id)?;
                if !grant.grant.can_read {
                    return Err(Error::Denied("shared draft read permission required"));
                }
                Ok(snapshot)
            })
            .await?;
            // Test-only deterministic held completion: the Store grant has
            // already been checked, but the new presence has not been admitted.
            // Exercise a concurrent real revoke before that final admission.
            #[cfg(test)]
            {
                let hold = lock(&team.0.multiplayer.held_presence_post)
                    .as_ref()
                    .filter(|hold| hold.client_id == command.client_id)
                    .cloned();
                if let Some(hold) = hold {
                    hold.reached.add_permits(1);
                    let _permit = hold.release.acquire_owned().await.map_err(|_| {
                        ApiError(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "presence test gate closed",
                        )
                    })?;
                }
            }
            if snapshot.state != aw_collab::DraftState::Active {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "shared draft generation is no longer active",
                ));
            }
            let utf16_len = snapshot.text.encode_utf16().count();
            if command.cursor_anchor as usize > utf16_len
                || command.cursor_head as usize > utf16_len
            {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "presence cursor is outside shared draft",
                ));
            }
            {
                let mut presence = lock(&team.0.multiplayer.presence);
                if admission_epoch == u64::MAX
                    || admission_epoch
                        != team
                            .0
                            .multiplayer
                            .presence_epoch
                            .load(AtomicOrdering::Acquire)
                {
                    return Err(ApiError(
                        StatusCode::CONFLICT,
                        "presence authority changed; refresh",
                    ));
                }
                cleanup_presence(&mut presence);
                let key = (
                    presence_draft_id.clone(),
                    presence_generation,
                    entry.user.id.clone(),
                    command.client_id.clone(),
                );
                if command.active {
                    let participant_count = presence
                        .keys()
                        .filter(|(_, _, participant, _)| participant == &entry.user.id)
                        .count();
                    if !presence.contains_key(&key)
                        && (presence.len() >= MAX_PRESENCE_RECORDS
                            || participant_count >= MAX_PRESENCE_PER_PARTICIPANT)
                    {
                        return Err(ApiError(
                            StatusCode::TOO_MANY_REQUESTS,
                            "presence view limit reached",
                        ));
                    }
                    presence.insert(
                        key,
                        PresenceRecord {
                            session_id: snapshot.target.session_id.clone(),
                            participant_id: entry.user.id.clone(),
                            display_name: entry.user.display_name.clone(),
                            cursor_anchor: command.cursor_anchor,
                            cursor_head: command.cursor_head,
                            updated_at: Instant::now(),
                        },
                    );
                } else {
                    presence.remove(&key);
                }
            }
            broadcast(
                team,
                &snapshot.target.session_id,
                "draft_presence_changed",
                serde_json::json!({
                    "draft_id": presence_draft_id,
                    "generation": presence_generation,
                }),
                true,
            );
            Ok(Json(serde_json::json!({"accepted": true})).into_response())
        }
        "/api/team/draft/presence/list" => {
            let command: DraftRefCommand = body_json(request).await?;
            let persistence = current(entry)?;
            let actor = persistence.actor.clone();
            let presence_draft_id = command.draft_id.clone();
            let presence_generation = command.generation;
            let grant = store_work(move || {
                let snapshot = persistence.store.shared_draft_snapshot(
                    &actor,
                    &command.draft_id,
                    command.generation,
                )?;
                let grant = persistence
                    .store
                    .draft_grant(&actor, &snapshot.target.session_id)?;
                Ok((snapshot, grant))
            })
            .await?;
            if !grant.1.grant.presence_read {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "presence read permission required",
                ));
            }
            // Old generations remain inspectable as history but have no live
            // presence, even for their original owner.
            if grant.0.state != aw_collab::DraftState::Active {
                return Ok(Json(serde_json::json!({"presence":[]})).into_response());
            }
            let mut presence = lock(&team.0.multiplayer.presence);
            cleanup_presence(&mut presence);
            let rows = presence
                .iter()
                .filter(|((draft_id, generation, _, _), _)| {
                    draft_id == &presence_draft_id && generation == &presence_generation
                })
                .map(|((_, _, _, client_id), value)| {
                    serde_json::json!({
                        "participant_id": value.participant_id,
                        "display_name": value.display_name,
                        "client_id": client_id,
                        "cursor_anchor": value.cursor_anchor,
                        "cursor_head": value.cursor_head,
                    })
                })
                .collect::<Vec<_>>();
            Ok(Json(serde_json::json!({"presence": rows})).into_response())
        }
        _ => Err(ApiError(
            StatusCode::NOT_FOUND,
            "collaboration route not found",
        )),
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}
