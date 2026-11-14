//! Canonical exact read-only SurfaceHost projection.
//!
//! Browser labels, paths and docking tickets never grant a Surface. Callers
//! retain a typed target and this module rechecks the owner, source, scope and
//! owner epoch before and after one bounded host read. Files are launch-root
//! only; immutable Reviews may retain an already-authorized Git scope.

use super::*;
use aw_surfaces::{Command, ViewRequest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Resource {
    File { path: String },
    Review { review_id: String },
}

impl Resource {
    fn request(&self) -> ViewRequest {
        match self {
            Self::File { path } => ViewRequest::File { path: path.clone() },
            Self::Review { review_id } => ViewRequest::Review {
                review_id: review_id.clone(),
            },
        }
    }

    pub(super) fn descriptor(&self) -> serde_json::Value {
        match self {
            Self::File { path } => serde_json::json!({"kind":"file", "path":path}),
            Self::Review { review_id } => {
                serde_json::json!({"kind":"review", "review_id":review_id})
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub source_id: String,
    pub scope_id: String,
    pub owner_epoch: String,
    pub resource: Resource,
}

impl Target {
    fn root_only(&self) -> bool {
        matches!(self.resource, Resource::File { .. })
    }
}

pub(super) fn bounded_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

pub(super) fn guarded_file_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn valid_resource(resource: &Resource) -> bool {
    match resource {
        Resource::File { path } => guarded_file_path(path),
        Resource::Review { review_id } => bounded_id(review_id),
    }
}

pub(super) fn current_with_host(
    app: &AppState,
    host: &aw_surfaces::SurfaceHost,
    target: &Target,
) -> ApiResult<()> {
    if !auth_alive(app) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "browser authorization expired",
        ));
    }
    if !bounded_id(&target.source_id)
        || !bounded_id(&target.scope_id)
        || !bounded_id(&target.owner_epoch)
        || !valid_resource(&target.resource)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid retained surface target",
        ));
    }
    let caps = host.capabilities();
    let active_epoch = lock(&app.0.session.state).snapshot.instance.clone();
    if target.source_id != caps.source_id
        || target.owner_epoch != active_epoch
        || (target.root_only() && target.scope_id != caps.view_root.id)
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "retained surface owner/source changed",
        ));
    }
    Ok(())
}

pub(super) fn current(app: &AppState, target: &Target) -> ApiResult<()> {
    let host = app.0.surfaces.as_ref().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "human workspace is not enabled",
    ))?;
    current_with_host(app, host, target)
}

/// Read one exact retained File or immutable Review through the original owner.
///
/// The returned projection is safe to hand to a higher-level capability layer
/// only after that layer performs its own final grant-generation revalidation.
pub(super) async fn read_with_host(
    app: &AppState,
    host: Arc<aw_surfaces::SurfaceHost>,
    target: &Target,
) -> ApiResult<serde_json::Value> {
    current_with_host(app, &host, target)?;
    let permit = app
        .0
        .surface_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "workspace readers busy"))?;
    let actor = app.0.session.actor.as_str().to_owned();
    let source = target.source_id.clone();
    let scope = target.scope_id.clone();
    let resource = target.resource.clone();
    let owner = app.clone();
    let worker_host = host.clone();
    let projected = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !auth_alive(&owner) {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "browser authorization expired",
            ));
        }
        let projected = worker_host
            .execute_if(
                &actor,
                Command::View {
                    source_id: source,
                    scope_id: scope,
                    request: resource.request(),
                },
                || auth_alive(&owner),
            )
            .map_err(|_| ApiError(StatusCode::FORBIDDEN, "original view no longer authorized"))?;
        if let Resource::Review { review_id } = &resource {
            if projected["result"]["kind"] != "review"
                || projected["result"]["id"].as_str() != Some(review_id.as_str())
            {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "retained review identity changed",
                ));
            }
        }
        if !auth_alive(&owner) {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "browser authorization expired",
            ));
        }
        Ok(projected)
    })
    .await
    .map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "workspace reader unavailable",
        )
    })??;
    current_with_host(app, &host, target)?;
    Ok(projected)
}

pub(super) async fn read(app: &AppState, target: &Target) -> ApiResult<serde_json::Value> {
    let host = app
        .0
        .surfaces
        .as_ref()
        .ok_or(ApiError(
            StatusCode::FORBIDDEN,
            "human workspace unavailable",
        ))?
        .clone();
    read_with_host(app, host, target).await
}
