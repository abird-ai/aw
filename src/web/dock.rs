//! Ephemeral, owner-mediated read-only File/Review handoff between browser
//! windows. A ticket is correlation only: every receiver read is authorized by
//! the original SurfaceHost, and no plaintext, grants or execution owner move.
use super::*;
use aw_surfaces::{Command, ViewRequest};
use axum::extract::rejection::JsonRejection;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;

const MAX_WINDOWS: usize = 24;
const MAX_TICKETS: usize = 24;
const WINDOW_TTL: Duration = Duration::from_secs(180);
const TICKET_TTL: Duration = Duration::from_secs(90);
/// Liveness is a short, receiver-proven observation, never proof the browser
/// cannot crash immediately afterwards. Origin must ask for an ACK *after*
/// it observes acceptance, avoiding an old registration as release authority.
const RECEIVER_ACK_TTL: Duration = Duration::from_secs(3);

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
    resource: Resource,
    presentation: Option<Presentation>,
    deadline: Instant,
    phase: Phase,
}
#[derive(Clone)]
enum Resource {
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
    fn descriptor(&self) -> serde_json::Value {
        match self {
            Self::File { path } => serde_json::json!({"kind":"file", "path":path}),
            Self::Review { review_id } => {
                serde_json::json!({"kind":"review","review_id":review_id})
            }
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Presentation {
    version: u8,
    fingerprint: String,
    query: String,
    selected: i32,
    focus: PresentationFocus,
    panes: Vec<PresentationPane>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum PresentationFocus {
    None,
    Find,
    Pane0,
    Pane1,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PresentationPane {
    anchor: u32,
    head: u32,
    from: u32,
    top: u32,
    left: u32,
}
impl Presentation {
    fn valid_for(&self, resource: &Resource) -> bool {
        const MAX_OFFSET: u32 = 4 * 1024 * 1024;
        self.version == 1
            && self.fingerprint.len() == 64
            && self
                .fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && self.query.len() <= 1024
            && self.query.encode_utf16().count() <= 512
            && !self.query.chars().any(char::is_control)
            && (-1..=4095).contains(&self.selected)
            && self.panes.len()
                == if matches!(resource, Resource::File { .. }) {
                    1
                } else {
                    2
                }
            && !(matches!(resource, Resource::File { .. })
                && matches!(self.focus, PresentationFocus::Pane1))
            && self.panes.iter().all(|pane| {
                pane.anchor <= MAX_OFFSET
                    && pane.head <= MAX_OFFSET
                    && pane.from <= MAX_OFFSET
                    && pane.top <= 32 * 1024 * 1024
                    && pane.left <= MAX_OFFSET
            })
    }
}

#[derive(Clone)]
enum Phase {
    Prepared,
    Accepted {
        receiver_id: String,
        confirmation_requested: bool,
        acknowledged_at: Option<Instant>,
    },
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
    #[cfg(test)]
    pub(super) fn stale_ack_for_test(&mut self, ticket: &str) {
        match &mut self.tickets.get_mut(ticket).unwrap().phase {
            Phase::Accepted {
                acknowledged_at, ..
            } => {
                *acknowledged_at =
                    Some(Instant::now() - RECEIVER_ACK_TTL - Duration::from_millis(10));
            }
            _ => panic!("test requires accepted receiver"),
        }
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
    #[serde(default)]
    presentation: Option<Presentation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrepareReview {
    window_id: String,
    window_proof: String,
    owner_epoch: String,
    source_id: String,
    scope_id: String,
    document_id: String,
    review_id: String,
    #[serde(default)]
    presentation: Option<Presentation>,
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
fn current(
    app: &AppState,
    source: &str,
    scope: &str,
    epoch: &str,
    root_only: bool,
) -> ApiResult<()> {
    let (active, root, actual_epoch) = enabled(app)?;
    if source != active || epoch != actual_epoch || (root_only && scope != root) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "docking owner/source changed",
        ));
    }
    Ok(())
}

/// Check the original, retained read through the bounded SurfaceHost worker.
/// A Review must be reauthorized by exact source, Git scope and immutable ID;
/// no inventory selection, browser path or worktree grant is synthesized.
async fn reauthorize_view(app: &AppState, ticket: &Ticket) -> ApiResult<()> {
    current(
        app,
        &ticket.source_id,
        &ticket.scope_id,
        &ticket.owner_epoch,
        matches!(ticket.resource, Resource::File { .. }),
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
    let resource = ticket.resource.clone();
    let presentation = ticket.presentation.clone();
    let owner = app.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !auth_alive(&owner) {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "browser authorization expired",
            ));
        }
        let projected = host
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
        // A source that changed after the sender captured its editor must
        // not release that view merely because the File is still readable.
        // The comparison is on the authenticated *owner* read, not a browser
        // label or the receiver's unsupervised assertion.
        if let (Resource::File { .. }, Some(expected)) = (&resource, &presentation) {
            let text = projected["result"]["text"].as_str().ok_or(ApiError(
                StatusCode::CONFLICT,
                "original File snapshot changed",
            ))?;
            let mut bytes = Vec::with_capacity(5 + text.len());
            bytes.extend_from_slice(b"file\0");
            bytes.extend_from_slice(text.as_bytes());
            let actual = hash(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if actual != expected.fingerprint {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "original File content changed",
                ));
            }
        }
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
        matches!(ticket.resource, Resource::File { .. }),
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

struct PrepareOrigin {
    window_id: String,
    window_proof: String,
    owner_epoch: String,
    source_id: String,
    scope_id: String,
    document_id: String,
    presentation: Option<Presentation>,
}
pub(super) async fn prepare(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Prepare>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    prepare_bound(
        &app,
        PrepareOrigin {
            window_id: request.window_id,
            window_proof: request.window_proof,
            owner_epoch: request.owner_epoch,
            source_id: request.source_id,
            scope_id: request.scope_id,
            document_id: request.document_id,
            presentation: request.presentation,
        },
        Resource::File { path: request.path },
    )
    .await
}
pub(super) async fn prepare_review(
    State(app): State<AppState>,
    payload: std::result::Result<Json<PrepareReview>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    prepare_bound(
        &app,
        PrepareOrigin {
            window_id: request.window_id,
            window_proof: request.window_proof,
            owner_epoch: request.owner_epoch,
            source_id: request.source_id,
            scope_id: request.scope_id,
            document_id: request.document_id,
            presentation: request.presentation,
        },
        Resource::Review {
            review_id: request.review_id,
        },
    )
    .await
}

fn bounded_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}
async fn prepare_bound(
    app: &AppState,
    origin: PrepareOrigin,
    resource: Resource,
) -> ApiResult<Response> {
    let PrepareOrigin {
        window_id,
        window_proof,
        owner_epoch,
        source_id,
        scope_id,
        document_id,
        presentation,
    } = origin;
    if !valid_token(&window_id)
        || !bounded_id(&scope_id)
        || !bounded_id(&source_id)
        || !document_id.starts_with("workspace-document-")
        || !bounded_id(&document_id)
        || match &resource {
            Resource::File { path } => !guarded_file_path(path),
            Resource::Review { review_id } => !bounded_id(review_id),
        }
        || presentation
            .as_ref()
            .is_some_and(|state| !state.valid_for(&resource))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid docking document descriptor",
        ));
    }
    let root_only = matches!(resource, Resource::File { .. });
    current(app, &source_id, &scope_id, &owner_epoch, root_only)?;
    {
        let mut registry = lock(&app.0.dock);
        registry.prune();
        registry.window(&window_id, &window_proof, &source_id, &owner_epoch)?;
    }
    let ticket_data = Ticket {
        origin_id: window_id,
        document_id,
        source_id,
        scope_id,
        owner_epoch,
        resource,
        presentation,
        deadline: Instant::now() + TICKET_TTL,
        phase: Phase::Prepared,
    };
    reauthorize_view(app, &ticket_data).await?;
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
        &window_proof,
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
        "ticket":ticket, "state":"prepared", "expires_in_ms":TICKET_TTL.as_millis()
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
    reauthorize_view(&app, &ticket).await?;
    // No file contents or owner credential. The receiver must issue its own
    // authenticated read and display before attempting accept.
    let mut descriptor = serde_json::json!({
        "source_id":ticket.source_id,
        "scope_id":ticket.scope_id, "owner_epoch":ticket.owner_epoch,
    });
    let resource = ticket.resource.descriptor();
    for (key, value) in resource
        .as_object()
        .expect("resource descriptor is an object")
    {
        descriptor[key] = value.clone();
    }
    if let Some(presentation) = &ticket.presentation {
        descriptor["presentation"] = serde_json::json!(presentation);
    }
    Ok(Json(descriptor).into_response())
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
    reauthorize_view(&app, &ticket).await?;
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
        confirmation_requested: false,
        acknowledged_at: None,
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

/// Source-initiated confirmation gate: the recipient must respond *after* this
/// authenticated request. Old accept replies and mere registration cannot
/// authorize origin closure; this is intentionally non-durable.
pub(super) async fn confirm(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, true)?;
    let mut registry = lock(&app.0.dock);
    registry.prune();
    registry.window(
        &request.window_id,
        &request.window_proof,
        &ticket.source_id,
        &ticket.owner_epoch,
    )?;
    let receiver_id = match &registry
        .tickets
        .get(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?
        .phase
    {
        Phase::Accepted { receiver_id, .. } => receiver_id.clone(),
        _ => return Err(ApiError(StatusCode::CONFLICT, "receiver has not accepted")),
    };
    if !registry
        .windows
        .get(&receiver_id)
        .is_some_and(|window| window.deadline > Instant::now())
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "receiver no longer available",
        ));
    }
    let held = registry
        .tickets
        .get_mut(&request.ticket)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "handoff unavailable"))?;
    if let Phase::Accepted {
        confirmation_requested,
        acknowledged_at,
        ..
    } = &mut held.phase
    {
        // Repeated confirmation may not invalidate a newly received ACK:
        // a second request is idempotent until ticket closure/expiration.
        if !*confirmation_requested {
            *confirmation_requested = true;
            *acknowledged_at = None;
        }
    }
    Ok(Json(serde_json::json!({"state":"confirming"})).into_response())
}

/// Only the accepted receiver's original window proof can ACK source's
/// confirmation challenge. The server verifies exact ticket, source, owner
/// epoch, authenticated browser, and live window on every observation.
pub(super) async fn heartbeat(
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
    let state = match &mut held.phase {
        Phase::Accepted {
            receiver_id,
            confirmation_requested,
            acknowledged_at,
        } if receiver_id == &request.window_id => {
            if *confirmation_requested {
                *acknowledged_at = Some(Instant::now());
                "accepted"
            } else {
                "confirming"
            }
        }
        Phase::Released => "released",
        _ => return Err(ApiError(StatusCode::CONFLICT, "receiver not accepted")),
    };
    Ok(Json(serde_json::json!({"state":state})).into_response())
}

/// Best-effort browser tab retirement (including pagehide with keepalive).
/// With a missed tab-close message the short ACK deadline still fails closed.
/// Old proofs cannot revive a retired window, and retiring one receiver never
/// retires a different Session/source/recipient.
pub(super) async fn retire(
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
    match &mut held.phase {
        Phase::Prepared => held.phase = Phase::Declined,
        Phase::Accepted { receiver_id, .. } if receiver_id == &request.window_id => (),
        Phase::Released => (),
        _ => {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "receiver has different owner",
            ))
        }
    }
    if let Some(window) = registry.windows.get_mut(&request.window_id) {
        window.deadline = Instant::now();
    }
    Ok(Json(serde_json::json!({"state":"retired"})).into_response())
}

pub(super) async fn status(
    State(app): State<AppState>,
    payload: std::result::Result<Json<Envelope>, JsonRejection>,
) -> ApiResult<Response> {
    let request = body(payload)?;
    let ticket = retrieve(&app, &request, true)?;
    let state = match &ticket.phase {
        Phase::Prepared => "prepared",
        Phase::Accepted {
            receiver_id,
            confirmation_requested,
            acknowledged_at,
        } if receiver_id != &ticket.origin_id => {
            // Do not instruct the source to close for an already-expired
            // receiving presentation. This is a lease, not a claim of perfect
            // process liveness after the last successful HTTP round trip.
            let registry = lock(&app.0.dock);
            if !registry
                .windows
                .get(receiver_id)
                .is_some_and(|w| w.deadline > Instant::now())
            {
                "receiver_unavailable"
            } else if !confirmation_requested {
                "confirming"
            } else if acknowledged_at
                .is_some_and(|time| Instant::now().duration_since(time) <= RECEIVER_ACK_TTL)
            {
                "accepted"
            } else {
                "confirming"
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
    reauthorize_view(&app, &ticket).await?;
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
        Phase::Accepted {
            receiver_id,
            confirmation_requested,
            acknowledged_at,
        } => {
            *confirmation_requested
                && acknowledged_at
                    .is_some_and(|time| Instant::now().duration_since(time) <= RECEIVER_ACK_TTL)
                && registry
                    .windows
                    .get(receiver_id)
                    .is_some_and(|w| w.deadline > Instant::now())
        }
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
