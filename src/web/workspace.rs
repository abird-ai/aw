//! Thin authenticated HTTP adapter for the blocking, UI-independent surface host.
//! No caller-selected roots, shell programs, grants, credentials or fetch proxy.
use super::*;
use aw_surfaces::{Command, Error as SurfaceError};

type Identity = Option<(String, String)>;
pub(super) struct WorkspaceError {
    error: ApiError,
    code: &'static str,
    identity: Identity,
}
impl IntoResponse for WorkspaceError {
    fn into_response(self) -> Response {
        if let Some((source, scope)) = self.identity {
            (self.error.0, Json(serde_json::json!({"error":self.error.1,"code":self.code,"source_id":source,"scope_id":scope}))).into_response()
        } else {
            self.error.into_response()
        }
    }
}
fn failure(error: SurfaceError, identity: Identity) -> WorkspaceError {
    let (status, message, code) = match error {
        SurfaceError::SourceChanged => (
            StatusCode::FORBIDDEN,
            "workspace source changed; refresh capabilities",
            "source_changed",
        ),
        SurfaceError::ScopeChanged(s) => (StatusCode::FORBIDDEN, s, "scope_denied"),
        SurfaceError::Denied(s) => (StatusCode::FORBIDDEN, s, "read_denied"),
        SurfaceError::Invalid(s) => (StatusCode::BAD_REQUEST, s, "invalid_request"),
        SurfaceError::Conflict(s) => (StatusCode::CONFLICT, s, "refresh_required"),
        SurfaceError::Limit(s) => (StatusCode::PAYLOAD_TOO_LARGE, s, "view_limit"),
        SurfaceError::Unavailable(s) => (StatusCode::SERVICE_UNAVAILABLE, s, "unavailable"),
    };
    WorkspaceError {
        error: ApiError(status, message),
        code,
        identity,
    }
}
fn plain(error: ApiError, identity: Identity) -> WorkspaceError {
    WorkspaceError {
        error,
        code: "request_failed",
        identity,
    }
}
fn view_identity(command: &Command) -> Identity {
    fn bounded(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    }
    match command {
        Command::View {
            source_id,
            scope_id,
            ..
        } if bounded(source_id) && bounded(scope_id) => Some((source_id.clone(), scope_id.clone())),
        _ => None,
    }
}

pub(super) async fn capabilities(State(app): State<AppState>) -> ApiResult<Response> {
    if !auth_alive(&app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
    }
    if app.0.surfaces.is_none() {
        return Ok(Json(serde_json::json!({"enabled":false,"instance":lock(&app.0.session.state).snapshot.instance,"hint":"start the owner with --web-workspace to grant human Files/Git views"})).into_response());
    }
    execute(&app, Command::Capabilities, true)
        .await
        .map_err(|error| error.error)
}

pub(super) async fn action(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Command>, axum::extract::rejection::JsonRejection>,
) -> std::result::Result<Response, WorkspaceError> {
    let Json(command) = payload.map_err(|_| {
        plain(
            ApiError(StatusCode::BAD_REQUEST, "invalid typed workspace command"),
            None,
        )
    })?;
    execute(&app, command, false).await
}

async fn execute(
    app: &AppState,
    command: Command,
    capabilities: bool,
) -> std::result::Result<Response, WorkspaceError> {
    let identity = view_identity(&command);
    let host = app.0.surfaces.clone().ok_or_else(|| {
        plain(
            ApiError(
                StatusCode::FORBIDDEN,
                "workspace services were not enabled by the owner",
            ),
            identity.clone(),
        )
    })?;
    let permit = app
        .0
        .surface_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            plain(
                ApiError(
                    StatusCode::TOO_MANY_REQUESTS,
                    "workspace workers busy; no action admitted",
                ),
                identity.clone(),
            )
        })?;
    let actor = app.0.session.actor.as_str().to_string();
    let executing = app.clone();
    let worker_identity = identity.clone();
    let body = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !auth_alive(&executing) { return Err(plain(ApiError(StatusCode::UNAUTHORIZED,"session expired"),worker_identity)); }
        let result = host.execute_if(&actor, command, ||auth_alive(&executing));
        if !auth_alive(&executing) { return Err(plain(ApiError(StatusCode::UNAUTHORIZED,"session expired before view disclosure"),worker_identity)); }
        let result = result.map_err(|error|failure(error,worker_identity.clone()))?;
        let value = if capabilities {
            serde_json::json!({"enabled":true,"instance":lock(&executing.0.session.state).snapshot.instance,"capabilities":result})
        } else { result };
        let body = serde_json::to_vec(&value).map_err(|_|plain(ApiError(StatusCode::SERVICE_UNAVAILABLE,"workspace response encoding failed"),worker_identity.clone()))?;
        if !auth_alive(&executing) { return Err(plain(ApiError(StatusCode::UNAUTHORIZED,"session expired before view disclosure"),worker_identity)); }
        Ok(body)
    }).await.map_err(|_|plain(ApiError(StatusCode::SERVICE_UNAVAILABLE,"workspace worker failed; inspect owner before retry"),identity.clone()))??;
    if !auth_alive(app) {
        return Err(plain(
            ApiError(
                StatusCode::UNAUTHORIZED,
                "session expired before view disclosure",
            ),
            identity,
        ));
    }
    Ok((
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response())
}

pub(super) async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../../web/workspace.js"),
    )
}
pub(super) async fn model() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../../web/workspace-model.js"),
    )
}
pub(super) async fn styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../web/workspace.css"),
    )
}
