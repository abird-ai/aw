//! Thin authenticated HTTP adapter for the blocking, UI-independent surface host.
//! No caller-selected roots, shell programs, grants, credentials or fetch proxy.
use super::*;
use aw_surfaces::{Command, Error as SurfaceError};
fn failure(e: SurfaceError) -> ApiError {
    match e {
        SurfaceError::Denied(s) => ApiError(StatusCode::FORBIDDEN, s),
        SurfaceError::Invalid(s) => ApiError(StatusCode::BAD_REQUEST, s),
        SurfaceError::Conflict(s) => ApiError(StatusCode::CONFLICT, s),
        SurfaceError::Limit(s) => ApiError(StatusCode::PAYLOAD_TOO_LARGE, s),
        SurfaceError::Unavailable(s) => ApiError(StatusCode::SERVICE_UNAVAILABLE, s),
    }
}
pub(super) async fn capabilities(
    State(app): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    match &app.0.surfaces {
        Some(host) => Ok(Json(
            serde_json::json!({"enabled":true,"instance":lock(&app.0.session.state).snapshot.instance,"capabilities":host.capabilities()}),
        )),
        None => Ok(Json(
            serde_json::json!({"enabled":false,"hint":"start the owner with --web-workspace to grant human Files/Git views"}),
        )),
    }
}
pub(super) async fn action(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Command>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Json(command) = payload
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid typed workspace command"))?;
    let host = app.0.surfaces.clone().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "workspace services were not enabled by the owner",
    ))?;
    let permit = app
        .0
        .surface_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "workspace workers busy; no action admitted",
            )
        })?;
    let actor = app.0.session.actor.as_str().to_string();
    let executing = app.clone();
    let value = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !auth_alive(&executing) {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
        }
        host.execute_if(&actor, command, || auth_alive(&executing))
            .map_err(failure)
    })
    .await
    .map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "workspace worker failed; inspect owner before retry",
        )
    })??;
    if !auth_alive(&app) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "session expired before view disclosure",
        ));
    }
    Ok(Json(value))
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
