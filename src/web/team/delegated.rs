//! Distinct-Actor browser docking for one exact delegated Agent branch.
//!
//! Team authentication selects both principals before this module runs. Browser
//! fields never choose an Actor. The short-lived docking ticket is correlation;
//! web::delegation remains the resource authority and is revalidated around
//! every read and immediately before response publication.

use super::*;
use crate::sessions::branch_view::{Reader, Target as AgentTarget};
#[cfg(feature = "surfaces")]
use crate::web::surface_read;
use axum::body::to_bytes;
use serde::{de::DeserializeOwned, Deserialize};
use std::collections::BTreeMap;

const MAX_BODY_BYTES: usize = 8192;
const MAX_WINDOWS: usize = 24;
const MAX_TICKETS: usize = 24;
const WINDOW_TTL: Duration = Duration::from_secs(180);
const TICKET_TTL: Duration = Duration::from_secs(90);
const RECEIVER_ACK_TTL: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct Registry {
    windows: BTreeMap<String, Window>,
    tickets: BTreeMap<String, Ticket>,
    #[cfg(test)]
    held_read: Option<std::sync::Arc<ReadHold>>,
}

#[cfg(test)]
pub(super) struct ReadHold {
    ready: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[cfg(test)]
impl ReadHold {
    pub(super) async fn ready(&self) {
        self.ready.notified().await;
    }

    pub(super) fn release(&self) {
        self.release.notify_one();
    }
}

#[cfg(test)]
impl Registry {
    pub(super) fn hold_next_read(&mut self) -> std::sync::Arc<ReadHold> {
        let hold = std::sync::Arc::new(ReadHold {
            ready: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        self.held_read = Some(hold.clone());
        hold
    }

    pub(super) fn stale_ack_for_test(&mut self, ticket: &str) {
        if let Some(Ticket {
            phase: Phase::Accepted {
                acknowledged_at, ..
            },
            ..
        }) = self.tickets.get_mut(ticket)
        {
            *acknowledged_at = Some(Instant::now() - RECEIVER_ACK_TTL - Duration::from_millis(10));
        }
    }
}

struct Window {
    proof_hash: [u8; 32],
    participant_id: String,
    principal: PrincipalId,
    store_id: String,
    source_epoch: String,
    deadline: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RouteKind {
    Agent,
    #[cfg(feature = "surfaces")]
    Surface,
}

#[derive(Clone)]
enum TicketResource {
    Agent(AgentTarget),
    #[cfg(feature = "surfaces")]
    Surface(surface_read::Target),
}

impl TicketResource {
    fn kind(&self) -> RouteKind {
        match self {
            Self::Agent(_) => RouteKind::Agent,
            #[cfg(feature = "surfaces")]
            Self::Surface(_) => RouteKind::Surface,
        }
    }
}

#[derive(Clone)]
struct Ticket {
    origin_id: String,
    receiver_id: Option<String>,
    owner_participant_id: String,
    recipient_participant_id: String,
    document_id: String,
    authority_store_id: String,
    source_epoch: String,
    resource: TicketResource,
    binding: delegation::Binding,
    grant_id: String,
    grant_generation: u64,
    deadline: Instant,
    read_generation: Option<u64>,
    phase: Phase,
}

#[derive(Clone)]
enum Phase {
    Prepared,
    Accepted {
        confirmation_requested: bool,
        acknowledged_at: Option<Instant>,
    },
    Released,
    Revoked,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    window_id: String,
    window_proof: String,
    store_id: String,
    source_epoch: String,
    session_id: String,
    branch_id: String,
    document_id: String,
    recipient_id: String,
}

#[cfg(feature = "surfaces")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareSurface {
    window_id: String,
    window_proof: String,
    store_id: String,
    source_epoch: String,
    source_id: String,
    scope_id: String,
    owner_epoch: String,
    document_id: String,
    recipient_id: String,
    kind: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    review_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receive {
    ticket: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    window_id: String,
    window_proof: String,
    ticket: String,
}

pub(super) fn is_route(path: &str, method: &Method) -> bool {
    method == Method::POST
        && matches!(
            path,
            "/api/dock/agent/register"
                | "/api/dock/agent/prepare"
                | "/api/dock/agent/receive"
                | "/api/dock/agent/read"
                | "/api/dock/agent/accept"
                | "/api/dock/agent/confirm"
                | "/api/dock/agent/heartbeat"
                | "/api/dock/agent/status"
                | "/api/dock/agent/release"
                | "/api/dock/agent/cancel"
                | "/api/dock/agent/revoke"
                | "/api/dock/surface/register"
                | "/api/dock/surface/prepare"
                | "/api/dock/surface/receive"
                | "/api/dock/surface/read"
                | "/api/dock/surface/accept"
                | "/api/dock/surface/confirm"
                | "/api/dock/surface/heartbeat"
                | "/api/dock/surface/status"
                | "/api/dock/surface/release"
                | "/api/dock/surface/cancel"
                | "/api/dock/surface/revoke"
        )
}

async fn body<T: DeserializeOwned>(request: Request) -> ApiResult<T> {
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), MAX_BODY_BYTES),
    )
    .await
    .map_err(|_| {
        ApiError(
            StatusCode::REQUEST_TIMEOUT,
            "delegated docking body timeout",
        )
    })?
    .map_err(|_| {
        ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "delegated docking body too large",
        )
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid delegated docking request"))
}

fn bounded_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-/@".contains(&byte))
}

fn prune(registry: &mut Registry) {
    let now = Instant::now();
    registry.windows.retain(|_, window| window.deadline > now);
    registry.tickets.retain(|_, ticket| {
        ticket.deadline > now && registry.windows.contains_key(&ticket.origin_id)
    });
}

fn team_entry<'a>(team: &'a TeamState, id: &str) -> ApiResult<&'a Entry> {
    team.0
        .entries
        .iter()
        .find(|entry| entry.user.id == id)
        .ok_or(ApiError(
            StatusCode::BAD_REQUEST,
            "unknown delegated docking participant",
        ))
}

fn store_and_epoch(entry: &Entry) -> ApiResult<(SessionStore, String, String)> {
    if !auth_alive(&entry.app) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "browser identity expired",
        ));
    }
    let store = entry.app.0.session.store.clone().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "durable Agent view authority unavailable",
    ))?;
    let store_id = store.store_id().map_err(api_error)?;
    let source_epoch = lock(&entry.app.0.session.state).snapshot.instance.clone();
    Ok((store, store_id, source_epoch))
}

fn agent_parent(
    owner: &Entry,
    target: &AgentTarget,
    source_epoch: &str,
) -> ApiResult<(SessionStore, delegation::ParentAuthority)> {
    let (store, store_id, current_epoch) = store_and_epoch(owner)?;
    if store_id != target.store_id || current_epoch != source_epoch {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated Agent source changed",
        ));
    }
    let session = store
        .session(&owner.app.0.session.actor, &target.session_id)
        .map_err(api_error)?;
    if session.owner != owner.app.0.session.actor {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "only the current Session owner may delegate this Agent view",
        ));
    }
    let reader =
        Reader::new(store.clone(), owner.app.0.session.actor.clone()).map_err(api_error)?;
    reader.authorize(target).map_err(api_error)?;
    let evidence = store
        .authorize_session_participation(
            &owner.app.0.session.actor,
            &target.session_id,
            aw_store::SessionParticipationAction::View,
        )
        .map_err(api_error)?;
    if !evidence.owner {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated Agent source is no longer owner-authorized",
        ));
    }
    Ok((
        store,
        delegation::ParentAuthority::Session {
            owner_membership_epoch: evidence.membership_epoch,
            audience_epoch: evidence.audience_epoch,
        },
    ))
}

fn window(
    registry: &Registry,
    id: &str,
    proof: &str,
    entry: &Entry,
    store_id: &str,
    source_epoch: &str,
) -> ApiResult<()> {
    if !valid_token(id) || !valid_token(proof) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated docking window unavailable",
        ));
    }
    let held = registry.windows.get(id).ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "delegated docking window unavailable",
    ))?;
    let digest = hash(proof.as_bytes());
    if held.deadline <= Instant::now()
        || held.participant_id != entry.user.id
        || held.principal != entry.app.0.session.actor
        || held.store_id != store_id
        || held.source_epoch != source_epoch
        || !bool::from(held.proof_hash.ct_eq(&digest))
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated docking window unavailable",
        ));
    }
    Ok(())
}

fn ticket_for_owner(
    team: &TeamState,
    entry: &Entry,
    request: &Envelope,
) -> ApiResult<(Ticket, Entry)> {
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    let ticket = registry
        .tickets
        .get(&request.ticket)
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "delegated handoff unavailable",
        ))?
        .clone();
    if ticket.owner_participant_id != entry.user.id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated handoff belongs to another owner",
        ));
    }
    window(
        &registry,
        &request.window_id,
        &request.window_proof,
        entry,
        &ticket.authority_store_id,
        &ticket.source_epoch,
    )?;
    if ticket.origin_id != request.window_id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated handoff origin changed",
        ));
    }
    let recipient = team_entry(team, &ticket.recipient_participant_id)?.clone();
    Ok((ticket, recipient))
}

fn ticket_for_recipient(
    team: &TeamState,
    entry: &Entry,
    request: &Envelope,
) -> ApiResult<(Ticket, Entry)> {
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    let ticket = registry
        .tickets
        .get(&request.ticket)
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "delegated handoff unavailable",
        ))?
        .clone();
    if ticket.recipient_participant_id != entry.user.id
        || ticket.binding.recipient != entry.app.0.session.actor
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated handoff belongs to another recipient",
        ));
    }
    let receiver_id = ticket.receiver_id.as_deref().ok_or(ApiError(
        StatusCode::CONFLICT,
        "delegated receiver has not registered",
    ))?;
    if receiver_id != request.window_id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated receiver window changed",
        ));
    }
    window(
        &registry,
        &request.window_id,
        &request.window_proof,
        entry,
        &ticket.authority_store_id,
        &ticket.source_epoch,
    )?;
    let owner = team_entry(team, &ticket.owner_participant_id)?.clone();
    Ok((ticket, owner))
}

#[cfg(feature = "surfaces")]
fn surface_parent(
    owner: &Entry,
    target: &surface_read::Target,
    source_epoch: &str,
) -> ApiResult<(SessionStore, delegation::ParentAuthority)> {
    let (store, _, current_epoch) = store_and_epoch(owner)?;
    if current_epoch != source_epoch {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated Surface browser owner changed",
        ));
    }
    let host = owner.delegated_surface.as_ref().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "delegated Surface source is not enabled for this owner",
    ))?;
    surface_read::current_with_host(&owner.app, host, target)?;
    Ok((
        store,
        delegation::ParentAuthority::Surface {
            owner_epoch: target.owner_epoch.clone(),
        },
    ))
}

fn ticket_parent(
    owner: &Entry,
    ticket: &Ticket,
) -> ApiResult<(SessionStore, delegation::ParentAuthority)> {
    let (store, parent) = match &ticket.resource {
        TicketResource::Agent(target) => agent_parent(owner, target, &ticket.source_epoch)?,
        #[cfg(feature = "surfaces")]
        TicketResource::Surface(target) => surface_parent(owner, target, &ticket.source_epoch)?,
    };
    if store.store_id().map_err(api_error)? != ticket.authority_store_id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated authority Store changed",
        ));
    }
    Ok((store, parent))
}

fn ensure_kind(ticket: &Ticket, expected: RouteKind) -> ApiResult<()> {
    if ticket.resource.kind() != expected {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated handoff resource kind does not match this route",
        ));
    }
    Ok(())
}

fn revalidate_grant(owner: &Entry, ticket: &Ticket) -> ApiResult<delegation::Witness> {
    let (store, parent) = ticket_parent(owner, ticket)?;
    if parent != ticket.binding.parent {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated parent authority changed",
        ));
    }
    let grants = lock(&owner.app.0.delegation);
    let witness = grants
        .authorize(&store, &ticket.grant_id, &ticket.binding)
        .map_err(api_error)?;
    if witness.generation() != ticket.grant_generation {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated grant generation changed",
        ));
    }
    Ok(witness)
}

async fn register(team: &TeamState, entry: &Entry, request: Request) -> ApiResult<Response> {
    let _: Empty = body(request).await?;
    let (_, store_id, source_epoch) = store_and_epoch(entry)?;
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    if registry.windows.len() >= MAX_WINDOWS {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many delegated docking windows",
        ));
    }
    let id = random_token().map_err(api_error)?;
    let proof = random_token().map_err(api_error)?;
    registry.windows.insert(
        id.clone(),
        Window {
            proof_hash: hash(proof.as_bytes()),
            participant_id: entry.user.id.clone(),
            principal: entry.app.0.session.actor.clone(),
            store_id: store_id.clone(),
            source_epoch: source_epoch.clone(),
            deadline: Instant::now() + WINDOW_TTL,
        },
    );
    Ok(Json(serde_json::json!({
        "window_id": id,
        "window_proof": proof,
        "store_id": store_id,
        "source_epoch": source_epoch,
        "expires_in_ms": WINDOW_TTL.as_millis(),
    }))
    .into_response())
}

async fn prepare(team: &TeamState, entry: &Entry, request: Request) -> ApiResult<Response> {
    let request: Prepare = body(request).await?;
    if !valid_token(&request.window_id)
        || !valid_token(&request.window_proof)
        || !bounded_id(&request.store_id)
        || !bounded_id(&request.source_epoch)
        || !bounded_id(&request.session_id)
        || !bounded_id(&request.branch_id)
        || !request.document_id.starts_with("agent-view-")
        || !bounded_id(&request.document_id)
        || request.recipient_id == entry.user.id
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid delegated Agent descriptor",
        ));
    }
    let recipient = team_entry(team, &request.recipient_id)?.clone();
    let target = AgentTarget {
        store_id: request.store_id,
        session_id: request.session_id,
        branch_id: request.branch_id,
    };
    {
        let mut registry = lock(&team.0.delegated);
        prune(&mut registry);
        window(
            &registry,
            &request.window_id,
            &request.window_proof,
            entry,
            &target.store_id,
            &request.source_epoch,
        )?;
        if registry.tickets.len() >= MAX_TICKETS {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "too many delegated handoffs",
            ));
        }
    }

    let (store, parent) = agent_parent(entry, &target, &request.source_epoch)?;
    let binding = delegation::Binding {
        owner: entry.app.0.session.actor.clone(),
        recipient: recipient.app.0.session.actor.clone(),
        source_id: target.store_id.clone(),
        site: entry.app.0.authority.clone(),
        resource: delegation::Resource::AgentBranch {
            store_id: target.store_id.clone(),
            session_id: target.session_id.clone(),
            branch_id: target.branch_id.clone(),
        },
        parent,
    };
    let issued = {
        let mut grants = lock(&entry.app.0.delegation);
        grants
            .grant(&store, binding.clone(), TICKET_TTL)
            .map_err(api_error)?
    };
    let ticket_id = random_token().map_err(api_error)?;
    let ticket = Ticket {
        origin_id: request.window_id,
        receiver_id: None,
        owner_participant_id: entry.user.id.clone(),
        recipient_participant_id: recipient.user.id.clone(),
        document_id: request.document_id,
        authority_store_id: target.store_id.clone(),
        source_epoch: request.source_epoch,
        resource: TicketResource::Agent(target),
        binding,
        grant_id: issued.id.clone(),
        grant_generation: issued.generation,
        deadline: Instant::now() + TICKET_TTL,
        read_generation: None,
        phase: Phase::Prepared,
    };
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    if registry.tickets.len() >= MAX_TICKETS {
        drop(registry);
        let mut grants = lock(&entry.app.0.delegation);
        let _ = grants.revoke(&store, &entry.app.0.session.actor, &issued.id);
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many delegated handoffs",
        ));
    }
    registry.tickets.insert(ticket_id.clone(), ticket);
    Ok(Json(serde_json::json!({
        "ticket": ticket_id,
        "state": "prepared",
        "grant_generation": issued.generation.to_string(),
        "expires_in_ms": TICKET_TTL.as_millis(),
    }))
    .into_response())
}

#[cfg(feature = "surfaces")]
fn surface_binding_resource(target: &surface_read::Target) -> delegation::Resource {
    match &target.resource {
        surface_read::Resource::File { path } => delegation::Resource::File {
            scope_id: target.scope_id.clone(),
            path: path.clone(),
        },
        surface_read::Resource::Review { review_id } => delegation::Resource::Review {
            scope_id: target.scope_id.clone(),
            review_id: review_id.clone(),
        },
    }
}

#[cfg(feature = "surfaces")]
async fn prepare_surface(team: &TeamState, entry: &Entry, request: Request) -> ApiResult<Response> {
    let request: PrepareSurface = body(request).await?;
    if !valid_token(&request.window_id)
        || !valid_token(&request.window_proof)
        || !bounded_id(&request.store_id)
        || !bounded_id(&request.source_epoch)
        || !surface_read::bounded_id(&request.source_id)
        || !surface_read::bounded_id(&request.scope_id)
        || !surface_read::bounded_id(&request.owner_epoch)
        || !request.document_id.starts_with("workspace-document-")
        || !surface_read::bounded_id(&request.document_id)
        || request.recipient_id == entry.user.id
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid delegated Surface descriptor",
        ));
    }
    let resource = match request.kind.as_str() {
        "file" => match (request.path, request.review_id) {
            (Some(path), None) => surface_read::Resource::File { path },
            _ => {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "delegated File requires only an exact path",
                ))
            }
        },
        "review" => match (request.path, request.review_id) {
            (None, Some(review_id)) => surface_read::Resource::Review { review_id },
            _ => {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "delegated Review requires only an exact immutable ID",
                ))
            }
        },
        _ => {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "delegated Surface kind must be file or review",
            ))
        }
    };
    if !surface_read::valid_resource(&resource) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid delegated Surface resource",
        ));
    }
    let recipient = team_entry(team, &request.recipient_id)?.clone();
    let target = surface_read::Target {
        source_id: request.source_id,
        scope_id: request.scope_id,
        owner_epoch: request.owner_epoch,
        resource,
    };
    {
        let mut registry = lock(&team.0.delegated);
        prune(&mut registry);
        window(
            &registry,
            &request.window_id,
            &request.window_proof,
            entry,
            &request.store_id,
            &request.source_epoch,
        )?;
        if registry.tickets.len() >= MAX_TICKETS {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "too many delegated handoffs",
            ));
        }
    }

    let host = entry.delegated_surface.as_ref().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "delegated Surface source is not enabled for this owner",
    ))?;
    // Prove the exact retained resource exists before issuing a grant. These
    // bytes remain owner-side; only the capability binding is returned.
    let _ = surface_read::read_with_host(&entry.app, host.clone(), &target).await?;
    let (store, parent) = surface_parent(entry, &target, &request.source_epoch)?;
    if store.store_id().map_err(api_error)? != request.store_id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated Surface authority Store changed",
        ));
    }
    let binding = delegation::Binding {
        owner: entry.app.0.session.actor.clone(),
        recipient: recipient.app.0.session.actor.clone(),
        source_id: target.source_id.clone(),
        site: entry.app.0.authority.clone(),
        resource: surface_binding_resource(&target),
        parent,
    };
    let issued = {
        let mut grants = lock(&entry.app.0.delegation);
        grants
            .grant(&store, binding.clone(), TICKET_TTL)
            .map_err(api_error)?
    };
    let ticket_id = random_token().map_err(api_error)?;
    let ticket = Ticket {
        origin_id: request.window_id,
        receiver_id: None,
        owner_participant_id: entry.user.id.clone(),
        recipient_participant_id: recipient.user.id.clone(),
        document_id: request.document_id,
        authority_store_id: request.store_id,
        source_epoch: request.source_epoch,
        resource: TicketResource::Surface(target),
        binding,
        grant_id: issued.id.clone(),
        grant_generation: issued.generation,
        deadline: Instant::now() + TICKET_TTL,
        read_generation: None,
        phase: Phase::Prepared,
    };
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    if registry.tickets.len() >= MAX_TICKETS {
        drop(registry);
        let mut grants = lock(&entry.app.0.delegation);
        let _ = grants.revoke(&store, &entry.app.0.session.actor, &issued.id);
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many delegated handoffs",
        ));
    }
    registry.tickets.insert(ticket_id.clone(), ticket);
    Ok(Json(serde_json::json!({
        "ticket": ticket_id,
        "state": "prepared",
        "grant_generation": issued.generation.to_string(),
        "expires_in_ms": TICKET_TTL.as_millis(),
    }))
    .into_response())
}

async fn receive(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Receive = body(request).await?;
    if !valid_token(&request.ticket) {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "delegated handoff unavailable",
        ));
    }
    let (ticket, owner) = {
        let mut registry = lock(&team.0.delegated);
        prune(&mut registry);
        let ticket = registry
            .tickets
            .get(&request.ticket)
            .ok_or(ApiError(
                StatusCode::NOT_FOUND,
                "delegated handoff unavailable",
            ))?
            .clone();
        if ticket.recipient_participant_id != entry.user.id
            || ticket.binding.recipient != entry.app.0.session.actor
            || !matches!(ticket.phase, Phase::Prepared)
            || ticket.receiver_id.is_some()
        {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "delegated handoff unavailable to this recipient",
            ));
        }
        ensure_kind(&ticket, expected)?;
        let owner = team_entry(team, &ticket.owner_participant_id)?.clone();
        (ticket, owner)
    };
    revalidate_grant(&owner, &ticket)?;
    let (_, store_id, source_epoch) = store_and_epoch(&owner)?;
    if store_id != ticket.authority_store_id || source_epoch != ticket.source_epoch {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated browser owner changed",
        ));
    }
    let id = random_token().map_err(api_error)?;
    let proof = random_token().map_err(api_error)?;
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    if registry.windows.len() >= MAX_WINDOWS {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many delegated docking windows",
        ));
    }
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    if !matches!(held.phase, Phase::Prepared)
        || held.receiver_id.is_some()
        || held.recipient_participant_id != entry.user.id
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "delegated handoff already received",
        ));
    }
    held.receiver_id = Some(id.clone());
    registry.windows.insert(
        id.clone(),
        Window {
            proof_hash: hash(proof.as_bytes()),
            participant_id: entry.user.id.clone(),
            principal: entry.app.0.session.actor.clone(),
            store_id: ticket.authority_store_id.clone(),
            source_epoch: ticket.source_epoch.clone(),
            deadline: Instant::now() + WINDOW_TTL,
        },
    );
    let resource = match &ticket.resource {
        TicketResource::Agent(target) => serde_json::json!({
            "kind": "agent_view",
            "store_id": target.store_id,
            "source_epoch": ticket.source_epoch,
            "session_id": target.session_id,
            "branch_id": target.branch_id,
        }),
        #[cfg(feature = "surfaces")]
        TicketResource::Surface(target) => {
            let mut descriptor = target.resource.descriptor();
            descriptor["source_id"] = target.source_id.clone().into();
            descriptor["scope_id"] = target.scope_id.clone().into();
            descriptor["owner_epoch"] = target.owner_epoch.clone().into();
            descriptor
        }
    };
    Ok(Json(serde_json::json!({
        "window_id": id,
        "window_proof": proof,
        "ticket": request.ticket,
        "resource": resource,
        "grant_generation": ticket.grant_generation.to_string(),
    }))
    .into_response())
}

async fn read_agent(team: &TeamState, entry: &Entry, request: Request) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, owner) = ticket_for_recipient(team, entry, &request)?;
    if matches!(ticket.phase, Phase::Revoked) {
        return Err(ApiError(StatusCode::FORBIDDEN, "delegated handoff revoked"));
    }
    let target = match &ticket.resource {
        TicketResource::Agent(target) => target.clone(),
        #[cfg(feature = "surfaces")]
        TicketResource::Surface(_) => {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "delegated resource is not an Agent view",
            ))
        }
    };
    let witness = revalidate_grant(&owner, &ticket)?;
    let (store, parent) = agent_parent(&owner, &target, &ticket.source_epoch)?;
    if parent != ticket.binding.parent {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated parent authority changed",
        ));
    }
    let permit = owner
        .app
        .0
        .branch_read_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "Agent view workers busy"))?;
    let worker_owner = owner.clone();
    let worker_target = target.clone();
    let source_epoch = ticket.source_epoch.clone();
    let binding = ticket.binding.clone();
    let witness_for_worker = witness.clone();
    let body = tokio::task::spawn_blocking(move || -> ApiResult<Vec<u8>> {
        let _permit = permit;
        let (_, before) = agent_parent(&worker_owner, &worker_target, &source_epoch)?;
        if before != binding.parent {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "delegated parent authority changed",
            ));
        }
        let reader = Reader::new(store.clone(), worker_owner.app.0.session.actor.clone())
            .map_err(api_error)?;
        let snapshot = reader.read(&worker_target).map_err(api_error)?;
        let encoded = agent_view::encode_snapshot(&worker_target, &source_epoch, &snapshot)?;
        let (_, after) = agent_parent(&worker_owner, &worker_target, &source_epoch)?;
        let grants = lock(&worker_owner.app.0.delegation);
        grants
            .revalidate(&store, &witness_for_worker, &after)
            .map_err(api_error)?;
        Ok(encoded)
    })
    .await
    .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "Agent view worker failed"))??;

    #[cfg(test)]
    {
        let held_read = { lock(&team.0.delegated).held_read.take() };
        if let Some(hold) = held_read {
            hold.ready.notify_one();
            hold.release.notified().await;
        }
    }

    let (store, parent) = agent_parent(&owner, &target, &ticket.source_epoch)?;
    let grants = lock(&owner.app.0.delegation);
    grants
        .revalidate(&store, &witness, &parent)
        .map_err(api_error)?;
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    if held.receiver_id.as_deref() != Some(request.window_id.as_str())
        || matches!(held.phase, Phase::Revoked)
        || held.grant_generation != ticket.grant_generation
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated handoff changed before publication",
        ));
    }
    held.read_generation = Some(ticket.grant_generation);
    // Linearize publication against explicit grant revoke: a revoke is ordered
    // either before this final check (denied) or after this response is built.
    grants
        .revalidate(&store, &witness, &parent)
        .map_err(api_error)?;
    Ok((
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response())
}

#[cfg(feature = "surfaces")]
async fn read_surface(team: &TeamState, entry: &Entry, request: Request) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, owner) = ticket_for_recipient(team, entry, &request)?;
    ensure_kind(&ticket, RouteKind::Surface)?;
    if matches!(ticket.phase, Phase::Revoked) {
        return Err(ApiError(StatusCode::FORBIDDEN, "delegated handoff revoked"));
    }
    let target = match &ticket.resource {
        TicketResource::Surface(target) => target.clone(),
        TicketResource::Agent(_) => {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "delegated resource is not a Surface view",
            ))
        }
    };
    let witness = revalidate_grant(&owner, &ticket)?;
    let (_, parent) = surface_parent(&owner, &target, &ticket.source_epoch)?;
    if parent != ticket.binding.parent {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated parent authority changed",
        ));
    }
    let host = owner.delegated_surface.as_ref().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "delegated Surface source is not enabled for this owner",
    ))?;
    let projected = surface_read::read_with_host(&owner.app, host.clone(), &target).await?;

    #[cfg(test)]
    {
        let held_read = { lock(&team.0.delegated).held_read.take() };
        if let Some(hold) = held_read {
            hold.ready.notify_one();
            hold.release.notified().await;
        }
    }

    let (store, parent) = surface_parent(&owner, &target, &ticket.source_epoch)?;
    let grants = lock(&owner.app.0.delegation);
    grants
        .revalidate(&store, &witness, &parent)
        .map_err(api_error)?;
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    if held.receiver_id.as_deref() != Some(request.window_id.as_str())
        || matches!(held.phase, Phase::Revoked)
        || held.grant_generation != ticket.grant_generation
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "delegated handoff changed before publication",
        ));
    }
    held.read_generation = Some(ticket.grant_generation);
    // Keep grant locked through response construction so explicit revoke is
    // ordered entirely before or after publication.
    grants
        .revalidate(&store, &witness, &parent)
        .map_err(api_error)?;
    Ok(Json(projected).into_response())
}

async fn accept(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, owner) = ticket_for_recipient(team, entry, &request)?;
    ensure_kind(&ticket, expected)?;
    revalidate_grant(&owner, &ticket)?;
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    if !matches!(held.phase, Phase::Prepared)
        || held.receiver_id.as_deref() != Some(request.window_id.as_str())
        || held.read_generation != Some(held.grant_generation)
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "recipient must complete the current delegated read before acceptance",
        ));
    }
    held.phase = Phase::Accepted {
        confirmation_requested: false,
        acknowledged_at: None,
    };
    Ok(Json(serde_json::json!({"state":"accepted"})).into_response())
}

async fn confirm(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, _) = ticket_for_owner(team, entry, &request)?;
    ensure_kind(&ticket, expected)?;
    revalidate_grant(entry, &ticket)?;
    let mut registry = lock(&team.0.delegated);
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    match &mut held.phase {
        Phase::Accepted {
            confirmation_requested,
            acknowledged_at,
        } => {
            if !*confirmation_requested {
                *confirmation_requested = true;
                *acknowledged_at = None;
            }
        }
        _ => {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "delegated receiver has not accepted",
            ))
        }
    }
    Ok(Json(serde_json::json!({"state":"confirming"})).into_response())
}

async fn heartbeat(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, owner) = ticket_for_recipient(team, entry, &request)?;
    ensure_kind(&ticket, expected)?;
    revalidate_grant(&owner, &ticket)?;
    let mut registry = lock(&team.0.delegated);
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    match &mut held.phase {
        Phase::Accepted {
            confirmation_requested,
            acknowledged_at,
        } if *confirmation_requested => {
            *acknowledged_at = Some(Instant::now());
            Ok(Json(serde_json::json!({"state":"accepted"})).into_response())
        }
        Phase::Accepted { .. } => {
            Ok(Json(serde_json::json!({"state":"confirming"})).into_response())
        }
        Phase::Released => Ok(Json(serde_json::json!({"state":"released"})).into_response()),
        Phase::Prepared | Phase::Revoked => Err(ApiError(
            StatusCode::CONFLICT,
            "delegated receiver is not accepted",
        )),
    }
}

async fn status(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, _) = ticket_for_owner(team, entry, &request)?;
    ensure_kind(&ticket, expected)?;
    let grant_alive = revalidate_grant(entry, &ticket).is_ok();
    let state = if !grant_alive {
        "revoked"
    } else {
        let registry = lock(&team.0.delegated);
        match &registry
            .tickets
            .get(&request.ticket)
            .ok_or(ApiError(
                StatusCode::NOT_FOUND,
                "delegated handoff unavailable",
            ))?
            .phase
        {
            Phase::Prepared => "prepared",
            Phase::Accepted {
                confirmation_requested,
                acknowledged_at,
            } => {
                let receiver_live = ticket
                    .receiver_id
                    .as_ref()
                    .and_then(|id| registry.windows.get(id))
                    .is_some_and(|window| window.deadline > Instant::now());
                if !receiver_live {
                    "receiver_unavailable"
                } else if *confirmation_requested
                    && acknowledged_at
                        .is_some_and(|time| Instant::now().duration_since(time) <= RECEIVER_ACK_TTL)
                {
                    "accepted"
                } else {
                    "confirming"
                }
            }
            Phase::Released => "released",
            Phase::Revoked => "revoked",
        }
    };
    Ok(Json(serde_json::json!({
        "state": state,
        "document_id": ticket.document_id,
    }))
    .into_response())
}

async fn release(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, _) = ticket_for_owner(team, entry, &request)?;
    ensure_kind(&ticket, expected)?;
    let witness = revalidate_grant(entry, &ticket)?;
    let (store, parent) = ticket_parent(entry, &ticket)?;
    let grants = lock(&entry.app.0.delegation);
    grants
        .revalidate(&store, &witness, &parent)
        .map_err(api_error)?;
    let mut registry = lock(&team.0.delegated);
    prune(&mut registry);
    let receiver_live = ticket
        .receiver_id
        .as_ref()
        .and_then(|id| registry.windows.get(id))
        .is_some_and(|window| window.deadline > Instant::now());
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    let confirmed = matches!(
        &held.phase,
        Phase::Accepted {
            confirmation_requested: true,
            acknowledged_at: Some(time),
        } if Instant::now().duration_since(*time) <= RECEIVER_ACK_TTL
    );
    if !receiver_live || !confirmed {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "delegated receiver no longer confirmed",
        ));
    }
    // Keep the grant fence held through the irreversible source-release state
    // change so concurrent revoke cannot win between validation and commit.
    grants
        .revalidate(&store, &witness, &parent)
        .map_err(api_error)?;
    held.phase = Phase::Released;
    Ok(Json(serde_json::json!({
        "state":"released",
        "document_id": held.document_id,
    }))
    .into_response())
}

async fn cancel(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, _) = ticket_for_owner(team, entry, &request)?;
    ensure_kind(&ticket, expected)?;
    {
        let mut registry = lock(&team.0.delegated);
        let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "delegated handoff unavailable",
        ))?;
        if !matches!(held.phase, Phase::Prepared) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "accepted delegated handoff cannot be silently rolled back",
            ));
        }
        held.phase = Phase::Revoked;
    }
    let store = entry.app.0.session.store.clone().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "durable Agent view authority unavailable",
    ))?;
    let mut grants = lock(&entry.app.0.delegation);
    grants
        .revoke(&store, &entry.app.0.session.actor, &ticket.grant_id)
        .map_err(api_error)?;
    Ok(Json(serde_json::json!({"state":"cancelled"})).into_response())
}

async fn revoke(
    team: &TeamState,
    entry: &Entry,
    request: Request,
    expected: RouteKind,
) -> ApiResult<Response> {
    let request: Envelope = body(request).await?;
    let (ticket, _) = ticket_for_owner(team, entry, &request)?;
    ensure_kind(&ticket, expected)?;
    let store = entry.app.0.session.store.clone().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "delegated browser authority Store unavailable",
    ))?;
    {
        let mut grants = lock(&entry.app.0.delegation);
        grants
            .revoke(&store, &entry.app.0.session.actor, &ticket.grant_id)
            .map_err(api_error)?;
    }
    let mut registry = lock(&team.0.delegated);
    let held = registry.tickets.get_mut(&request.ticket).ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "delegated handoff unavailable",
    ))?;
    held.phase = Phase::Revoked;
    Ok(Json(serde_json::json!({"state":"revoked"})).into_response())
}

pub(super) async fn route(
    team: &TeamState,
    entry: &Entry,
    path: &str,
    request: Request,
) -> ApiResult<Response> {
    match path {
        "/api/dock/agent/register" => register(team, entry, request).await,
        "/api/dock/agent/prepare" => prepare(team, entry, request).await,
        "/api/dock/agent/receive" => receive(team, entry, request, RouteKind::Agent).await,
        "/api/dock/agent/read" => read_agent(team, entry, request).await,
        "/api/dock/agent/accept" => accept(team, entry, request, RouteKind::Agent).await,
        "/api/dock/agent/confirm" => confirm(team, entry, request, RouteKind::Agent).await,
        "/api/dock/agent/heartbeat" => heartbeat(team, entry, request, RouteKind::Agent).await,
        "/api/dock/agent/status" => status(team, entry, request, RouteKind::Agent).await,
        "/api/dock/agent/release" => release(team, entry, request, RouteKind::Agent).await,
        "/api/dock/agent/cancel" => cancel(team, entry, request, RouteKind::Agent).await,
        "/api/dock/agent/revoke" => revoke(team, entry, request, RouteKind::Agent).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/register" => register(team, entry, request).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/prepare" => prepare_surface(team, entry, request).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/receive" => receive(team, entry, request, RouteKind::Surface).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/read" => read_surface(team, entry, request).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/accept" => accept(team, entry, request, RouteKind::Surface).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/confirm" => confirm(team, entry, request, RouteKind::Surface).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/heartbeat" => heartbeat(team, entry, request, RouteKind::Surface).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/status" => status(team, entry, request, RouteKind::Surface).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/release" => release(team, entry, request, RouteKind::Surface).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/cancel" => cancel(team, entry, request, RouteKind::Surface).await,
        #[cfg(feature = "surfaces")]
        "/api/dock/surface/revoke" => revoke(team, entry, request, RouteKind::Surface).await,
        _ => Err(ApiError(
            StatusCode::NOT_FOUND,
            "delegated docking route unavailable",
        )),
    }
}
