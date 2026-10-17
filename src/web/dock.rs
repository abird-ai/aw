//! Ephemeral, owner-mediated File-view handoff between separately mounted browser
//! windows. A ticket is correlation only: every receiver read is authorized by
//! the original SurfaceHost, and no plaintext, grants or execution owner move.
use super::*;
use aw_surfaces::{Command, ViewRequest};
use axum::extract::rejection::JsonRejection;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

const MAX_WINDOWS: usize = 24;
const MAX_TICKETS: usize = 24;
const WINDOW_TTL: Duration = Duration::from_secs(180);
const TICKET_TTL: Duration = Duration::from_secs(90);

#[derive(Default)]
pub(super) struct Registry {
    windows: BTreeMap<String, Window>,
    tickets: BTreeMap<String, Ticket>,
}
struct Window {
    proof_hash: [u8; 32],
    source_id: String,
    owner_epoch: String,
    deadline: Instant,
}
#[derive(Clone)]
struct Ticket {
    origin_id: String,
    document_id: String,
    source_id: String,
    scope_id: String,
    owner_epoch: String,
    path: String,
    deadline: Instant,
    phase: Phase,
}
#[derive(Clone)]
enum Phase {
    Prepared,
    Accepted { receiver_id: String },
    Declined,
    Released,
}
impl Registry {
    #[cfg(test)]
    pub(super) fn expire_ticket_for_test(&mut self, ticket: &str) {
        self.tickets.get_mut(ticket).unwrap().deadline = Instant::now() - Duration::from_millis(1);
    }
    #[cfg(test)]
    pub(super) fn expire_window_for_test(&mut self, window: &str) {
        self.windows.get_mut(window).unwrap().deadline = Instant::now() - Duration::from_millis(1);
    }
    fn prune(&mut self) {
        let now = Instant::now();
        self.windows.retain(|_, window| window.deadline > now);
        self.tickets.retain(|_, ticket| {
            ticket.deadline > now && self.windows.contains_key(&ticket.origin_id)
        });
    }
    fn window(&self, id: &str, proof: &str, source: &str, epoch: &str) -> ApiResult<()> {
        if !valid_token(id) || !valid_token(proof) {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "presentation window unavailable",
            ));
        }
        let window = self.windows.get(id).ok_or(ApiError(
            StatusCode::FORBIDDEN,
            "presentation window unavailable",
        ))?;
        let digest = hash(proof.as_bytes());
        if window.deadline <= Instant::now()
            || window.source_id != source
            || window.owner_epoch != epoch
            || !bool::from(window.proof_hash.ct_eq(&digest))
        {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "presentation window unavailable",
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prepare {
    window_id: String,
    window_proof: String,
    owner_epoch: String,
    source_id: String,
    scope_id: String,
    document_id: String,
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    window_id: String,
    window_proof: String,
    ticket: String,
}

fn body<T: DeserializeOwned>(payload: std::result::Result<Json<T>, JsonRejection>) -> ApiResult<T> {
    payload
        .map(|Json(value)| value)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid bounded docking request"))
}

fn enabled(app: &AppState) -> ApiResult<(String, String, String)> {
    if !auth_alive(app) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "browser authorization expired",
        ));
    }
    let host = app.0.surfaces.as_ref().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "human workspace is not enabled",
    ))?;
    let caps = host.capabilities();
    let epoch = lock(&app.0.session.state).snapshot.instance.clone();
    Ok((caps.source_id, caps.view_root.id, epoch))
}
fn guarded_file_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
fn current(app: &AppState, source: &str, scope: &str, epoch: &str) -> ApiResult<()> {
    let (active, root, actual_epoch) = enabled(app)?;
    if source != active || scope != root || epoch != actual_epoch {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "docking owner/source changed",
        ));
    }
    Ok(())
}

/// Check the *actual* retained Files read through the existing bounded resource
/// executor. Never return those bytes in a ticket/inspect/accept response.
async fn reauthorize_file(app: &AppState, ticket: &Ticket) -> ApiResult<()> {
    current(
        app,
        &ticket.source_id,
        &ticket.scope_id,
        &ticket.owner_epoch,
    )?;
    let host = app
        .0
        .surfaces
        .as_ref()
        .ok_or(ApiError(
            StatusCode::FORBIDDEN,
            "human workspace unavailable",
        ))?
        .clone();
    let permit = app
        .0
        .surface_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "workspace readers busy"))?;
    let actor = app.0.session.actor.as_str().to_owned();
    let source = ticket.source_id.clone();
    let scope = ticket.scope_id.clone();
    let path = ticket.path.clone();
    let owner = app.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !auth_alive(&owner) {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "browser authorization expired",
            ));
        }
        host.execute_if(
            &actor,
            Command::View {
                source_id: source,
                scope_id: scope,
                request: ViewRequest::File { path },
            },
            || auth_alive(&owner),
        )
        .map_err(|_| ApiError(StatusCode::FORBIDDEN, "original File no longer authorized"))?;
        if !auth_alive(&owner) {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "browser authorization expired",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "workspace reader unavailable",
        )
    })??;
    current(
        app,
        &ticket.source_id,
        &ticket.scope_id,
        &ticket.owner_epoch,
    )
}

pub(super) async fn register(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Empty>, JsonRejection>,
) -> ApiResult<Response> {
    let _ = body(payload)?;
    let (source, scope, epoch) = enabled(&app)?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    if registry.windows.len() >= MAX_WINDOWS {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many docking windows",
        ));
    }
    let id = random_token().map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "presentation identity unavailable",
        )
    })?;
    let proof = random_token().map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "presentation identity unavailable",
        )
    })?;
    registry.windows.insert(
        id.clone(),
        Window {
            proof_hash: hash(proof.as_bytes()),
            source_id: source.clone(),
            owner_epoch: epoch.clone(),
            deadline: Instant::now() + WINDOW_TTL,
        },
    );
    Ok(Json(serde_json::json!({
        "window_id": id, "window_proof": proof,
        "source_id": source, "scope_id": scope, "owner_epoch": epoch,
        "expires_in_ms": WINDOW_TTL.as_millis(),
    }))
    .into_response())
}

pub(super) async fn prepare(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Prepare>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    if !valid_token(&request.window_id)
        || !guarded_file_path(&request.path)
        || request.document_id.len() > 128
        || !request.document_id.starts_with("workspace-document-")
        || !request
            .document_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid docking File descriptor",
        ));
    }
    let (_, root, _) = enabled(&app)?;
    current(
        &app,
        &request.source_id,
        &request.scope_id,
        &request.owner_epoch,
    )?;
    if request.scope_id != root {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Files require the original launch scope",
        ));
    }
    {
        let mut registry = lock(&app.0.dock);
        registry.prune();
        registry.window(
            &request.window_id,
            &request.window_proof,
            &request.source_id,
            &request.owner_epoch,
        )?;
    }
    let ticket_data = Ticket {
        origin_id: request.window_id,
        document_id: request.document_id,
        source_id: request.source_id,
        scope_id: request.scope_id,
        owner_epoch: request.owner_epoch,
        path: request.path,
        deadline: Instant::now() + TICKET_TTL,
        phase: Phase::Prepared,
    };
    reauthorize_file(&app, &ticket_data).await?;
    let ticket = random_token().map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "handoff identity unavailable",
        )
    })?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    registry.window(
        &ticket_data.origin_id,
        &request.window_proof,
        &ticket_data.source_id,
        &ticket_data.owner_epoch,
    )?;
    if registry.tickets.len() >= MAX_TICKETS {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many pending handoffs",
        ));
    }
    registry.tickets.insert(ticket.clone(), ticket_data);
    Ok(Json(serde_json::json!({
        "ticket": ticket, "state":"prepared",
        "expires_in_ms":TICKET_TTL.as_millis(),
    }))
    .into_response())
}

fn retrieve(app: &AppState, request: &Envelope, origin: bool) -> ApiResult<Ticket> {
    if !valid_token(&request.ticket) {
        return Err(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"));
    }
    let (source, _, epoch) = enabled(app)?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    registry.window(&request.window_id, &request.window_proof, &source, &epoch)?;
    let ticket = registry
        .tickets
        .get(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?;
    if ticket.source_id != source
        || ticket.owner_epoch != epoch
        || (origin && ticket.origin_id != request.window_id)
        || (!origin && ticket.origin_id == request.window_id)
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "handoff window or source unavailable",
        ));
    }
    Ok(ticket.clone())
}

pub(super) async fn inspect(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, false)?;
    if !matches!(ticket.phase, Phase::Prepared) {
        return Err(ApiError(StatusCode::CONFLICT, "handoff already accepted"));
    }
    reauthorize_file(&app, &ticket).await?;
    // No file contents or owner credential. The receiver must issue its own
    // authenticated read and display before attempting accept.
    Ok(Json(serde_json::json!({
        "kind":"file", "source_id":ticket.source_id,
        "scope_id":ticket.scope_id, "owner_epoch":ticket.owner_epoch,
        "path":ticket.path,
    }))
    .into_response())
}

pub(super) async fn accept(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, false)?;
    if !matches!(ticket.phase, Phase::Prepared) {
        return Err(ApiError(StatusCode::CONFLICT, "handoff already accepted"));
    }
    reauthorize_file(&app, &ticket).await?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    registry.window(
        &request.window_id,
        &request.window_proof,
        &ticket.source_id,
        &ticket.owner_epoch,
    )?;
    let held = registry
        .tickets
        .get_mut(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?;
    if !matches!(held.phase, Phase::Prepared) {
        return Err(ApiError(StatusCode::CONFLICT, "handoff already accepted"));
    }
    held.phase = Phase::Accepted {
        receiver_id: request.window_id,
    };
    Ok(Json(serde_json::json!({"state":"accepted"})).into_response())
}

/// A receiver that failed its own read/render may reject Prepared without
/// hiding its failure for 90 seconds. It cannot revoke an already Accepted
/// receiver; the source remains authoritative for final Release/Cancel.
pub(super) async fn decline(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, false)?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    registry.window(
        &request.window_id,
        &request.window_proof,
        &ticket.source_id,
        &ticket.owner_epoch,
    )?;
    let held = registry
        .tickets
        .get_mut(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?;
    if !matches!(held.phase, Phase::Prepared) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "handoff is no longer pending",
        ));
    }
    held.phase = Phase::Declined;
    Ok(Json(serde_json::json!({"state":"declined"})).into_response())
}

pub(super) async fn status(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, true)?;
    let state = match &ticket.phase {
        Phase::Prepared => "prepared",
        Phase::Accepted { receiver_id } if receiver_id != &ticket.origin_id => {
            // Do not instruct the source to close for an already-expired
            // receiving presentation. This is a lease, not a claim of perfect
            // process liveness after the last successful HTTP round trip.
            let registry = lock(&app.0.dock);
            if registry
                .windows
                .get(receiver_id)
                .is_some_and(|w| w.deadline > Instant::now())
            {
                "accepted"
            } else {
                "receiver_unavailable"
            }
        }
        Phase::Accepted { .. } => "unavailable",
        Phase::Declined => "declined",
        Phase::Released => "released",
    };
    Ok(Json(serde_json::json!({
        "state":state, "document_id":ticket.document_id,
    }))
    .into_response())
}

pub(super) async fn release(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, true)?;
    // An earlier accepted read is not a perpetual File grant. Recheck
    // current browser auth and physically pinned source before release.
    reauthorize_file(&app, &ticket).await?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    registry.window(
        &request.window_id,
        &request.window_proof,
        &ticket.source_id,
        &ticket.owner_epoch,
    )?;
    let receiver_live = match &registry
        .tickets
        .get(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?
        .phase
    {
        Phase::Accepted { receiver_id } => registry
            .windows
            .get(receiver_id)
            .is_some_and(|w| w.deadline > Instant::now()),
        _ => false,
    };
    if !receiver_live {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "receiver no longer confirmed",
        ));
    }
    let held = registry
        .tickets
        .get_mut(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?;
    match held.phase {
        Phase::Accepted { .. } => held.phase = Phase::Released,
        Phase::Released => return Err(ApiError(StatusCode::CONFLICT, "handoff already released")),
        Phase::Prepared | Phase::Declined => {
            return Err(ApiError(StatusCode::CONFLICT, "receiver has not accepted"))
        }
    }
    Ok(
        Json(serde_json::json!({"state":"released","document_id":ticket.document_id}))
            .into_response(),
    )
}

pub(super) async fn cancel(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, true)?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    let held = registry
        .tickets
        .get(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?;
    if !matches!(held.phase, Phase::Prepared | Phase::Declined)
        || ticket.origin_id != request.window_id
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "accepted handoff cannot be silently rolled back",
        ));
    }
    registry.tickets.remove(&request.ticket);
    Ok(Json(serde_json::json!({"state":"cancelled"})).into_response())
}
