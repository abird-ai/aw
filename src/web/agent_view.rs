//! Authenticated read-only branches from the owner's already-open SQLite Store.
//! No Session creation, activation, State reopen, provider or tool dispatch.
use super::*;
use crate::sessions::branch_view::{Reader, Target};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const EXPIRES_IN_MS: u32 = 15_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadRequest {
    store_id: String,
    source_epoch: String,
    session_id: String,
    branch_id: String,
}
#[derive(Serialize)]
struct ReadResponse<'a> {
    store_id: &'a str,
    source_epoch: &'a str,
    session_id: &'a str,
    branch_id: &'a str,
    title: &'a str,
    branch_name: &'a str,
    head: &'a str,
    revision: String,
    turns: &'a [Turn],
    context_blocks: &'a [aw_agent_core::ContextBlock],
    needs_inspection: bool,
    retained_bytes: usize,
    expires_in_ms: u32,
}
fn live(app: &AppState, source_epoch: &str) -> ApiResult<()> {
    if !auth_alive(app) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "session expired before view disclosure",
        ));
    }
    if lock(&app.0.session.state).snapshot.instance != source_epoch {
        return Err(ApiError(StatusCode::FORBIDDEN, "Agent view source changed"));
    }
    Ok(())
}
fn slot(app: &AppState) -> ApiResult<OwnedSemaphorePermit> {
    app.0
        .branch_read_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Agent view workers busy; refresh explicitly",
            )
        })
}
fn authorize(
    app: &AppState,
    store: &SessionStore,
    reader: &Reader,
    target: &Target,
    source_epoch: &str,
) -> ApiResult<()> {
    live(app, source_epoch)?;
    if app.0.owned_sessions_only
        && store
            .session(&app.0.session.actor, &target.session_id)
            .map_err(api_error)?
            .owner
            != app.0.session.actor
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "this participant can read only its own private history",
        ));
    }
    reader.authorize(target).map_err(api_error)
}

// Include the size field itself in the client budget and reject the full DTO.
fn encode(response: &mut ReadResponse<'_>) -> ApiResult<Vec<u8>> {
    for _ in 0..4 {
        let encoded = serde_json::to_vec(response).map_err(|_| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Agent view could not be encoded",
            )
        })?;
        if encoded.len() > MAX_RESPONSE_BYTES {
            return Err(ApiError(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Agent view exceeds the bounded transcript budget",
            ));
        }
        if response.retained_bytes == encoded.len() {
            return Ok(encoded);
        }
        response.retained_bytes = encoded.len();
    }
    Err(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Agent view size could not be determined",
    ))
}

pub(super) async fn read(
    State(app): State<AppState>,
    payload: std::result::Result<Json<ReadRequest>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Response> {
    let Json(request) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid Agent view request"))?;
    if [
        &request.store_id,
        &request.source_epoch,
        &request.session_id,
        &request.branch_id,
    ]
    .iter()
    .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid exact Agent view identity",
        ));
    }
    live(&app, &request.source_epoch)?;
    let store = current_store(&app)?;
    let permit = slot(&app)?;
    let owner = app.clone();
    let response_epoch = request.source_epoch.clone();
    let body = tokio::task::spawn_blocking(move || -> ApiResult<Vec<u8>> {
        let _permit = permit;
        let reader =
            Reader::new(store.clone(), owner.0.session.actor.clone()).map_err(api_error)?;
        let target = Target {
            store_id: request.store_id,
            session_id: request.session_id,
            branch_id: request.branch_id,
        };
        authorize(&owner, &store, &reader, &target, &request.source_epoch)?;
        let snapshot = reader.read(&target).map_err(api_error)?;
        let mut response = ReadResponse {
            store_id: &target.store_id,
            source_epoch: &request.source_epoch,
            session_id: &target.session_id,
            branch_id: &target.branch_id,
            title: &snapshot.title,
            branch_name: &snapshot.branch_name,
            head: &snapshot.head,
            revision: snapshot.revision.to_string(),
            turns: snapshot.turns.as_ref(),
            context_blocks: &snapshot.context_blocks,
            needs_inspection: snapshot.needs_inspection,
            retained_bytes: 0,
            expires_in_ms: EXPIRES_IN_MS,
        };
        let encoded = encode(&mut response)?;
        authorize(&owner, &store, &reader, &target, &request.source_epoch)?;
        Ok(encoded)
    })
    .await
    .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "Agent view worker failed"))??;
    live(&app, &response_epoch)?;
    Ok((
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response())
}

pub(super) async fn catalog(app: &AppState) -> ApiResult<Response> {
    let source_epoch = lock(&app.0.session.state).snapshot.instance.clone();
    live(app, &source_epoch)?;
    let Some(store) = app.0.session.store.clone() else {
        return Ok(Json(
            serde_json::json!({"store_id":null,"source_epoch":source_epoch,"sessions":[]}),
        )
        .into_response());
    };
    let permit = slot(app)?;
    let owner = app.clone();
    let response_epoch = source_epoch.clone();
    let body = tokio::task::spawn_blocking(move || -> ApiResult<Vec<u8>> {
        let _permit = permit;
        live(&owner, &source_epoch)?;
        let actor = &owner.0.session.actor;
        let store_id = store.store_id().map_err(api_error)?;
        let mut rows = Vec::new();
        let mut sessions = Vec::new();
        for session in store.list(actor).map_err(api_error)?.into_iter().take(512) {
            if owner.0.owned_sessions_only && session.owner != *actor { continue; }
            let mut branches = store.branches(actor, &session.id).map_err(api_error)?.into_iter().take(128).collect::<Vec<_>>();
            for branch in &mut branches { branch.name = crate::presentation::safe_text(&branch.name); }
            sessions.push(session.id.clone());
            rows.push(serde_json::json!({
                "id":session.id,
                "title":crate::presentation::safe_text(&session.title),
                "updated_at":session.updated_at,
                "imported":session.imported,
                "directory":crate::presentation::safe_text(&session.binding.working_directory.display().to_string()),
                "branches":branches
            }));
        }
        let body = serde_json::to_vec(&serde_json::json!({"store_id":store_id,"source_epoch":source_epoch,"sessions":rows}))
            .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "Agent catalog could not be encoded"))?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "Agent catalog exceeds the bounded metadata budget"));
        }
        // Discovery preserves authorized metadata for remote targets. Only the
        // transcript endpoint requires a local execution target; no fallback.
        for id in sessions {
            if owner.0.owned_sessions_only && store.session(actor, &id).map_err(api_error)?.owner != *actor {
                return Err(ApiError(StatusCode::FORBIDDEN, "Agent catalog authority changed"));
            }
            store.branches(actor, &id).map_err(api_error)?;
        }
        if store.store_id().map_err(api_error)? != store_id {
            return Err(ApiError(StatusCode::FORBIDDEN, "Agent catalog Store changed"));
        }
        live(&owner, &source_epoch)?;
        Ok(body)
    }).await.map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "Agent catalog worker failed"))??;
    live(app, &response_epoch)?;
    Ok((
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoded_budget_includes_envelope_and_revision_is_lossless() {
        let mut response = ReadResponse {
            store_id: "store",
            source_epoch: "owner",
            session_id: "session",
            branch_id: "branch",
            title: "title",
            branch_name: "main",
            head: "head",
            revision: u64::MAX.to_string(),
            turns: &[],
            context_blocks: &[],
            needs_inspection: false,
            retained_bytes: 0,
            expires_in_ms: EXPIRES_IN_MS,
        };
        let baseline = encode(&mut response).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&baseline).unwrap();
        assert_eq!(parsed["revision"], "18446744073709551615");
        assert_eq!(parsed["retained_bytes"], baseline.len());
        let large = "x".repeat(MAX_RESPONSE_BYTES - baseline.len() + response.title.len());
        response.title = &large;
        response.retained_bytes = 0;
        // The raw content fits its nominal limit, but wrapper/size digits count.
        assert!(large.len() < MAX_RESPONSE_BYTES);
        assert_eq!(
            encode(&mut response).unwrap_err().0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
