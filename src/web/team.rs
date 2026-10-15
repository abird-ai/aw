//! Invite-only named participants using the existing web/Agent implementation.
//! The gateway authenticates a random cookie BEFORE choosing a participant's
//! immutable AppState. No browser field/header can choose an actor, root or key.
//! Numeric loopback only. Each participant owns private sessions; no peer sharing,
//! filesystem views, tools or terminals are exposed by this first team slice.
use super::*;
use aw_identity::{Participant, Roster};
use axum::body::to_bytes;
use tower::ServiceExt;

#[cfg(feature = "multiplayer")]
mod multiplayer;

const MAX_PAIR_BYTES: usize = 1024;
const MAX_GATEWAY_INFLIGHT: usize = 16;

/// Trusted Rust embedding API: authenticate users at the gateway, not by passing
/// a caller-controlled principal string into this constructor.
pub struct ParticipantRuntime {
    pub participant_id: String,
    pub agent: Agent,
    pub persistence: PersistentSession,
}
#[derive(Clone)]
struct Entry {
    user: Participant,
    app: AppState,
    router: Router,
}
#[derive(Clone)]
struct TeamState(Arc<TeamInner>);
struct TeamInner {
    realm: String,
    entries: Vec<Entry>,
    pairing_failures: Mutex<Vec<Instant>>,
    requests: Arc<Semaphore>,
    stop: CancellationToken,
    #[cfg(feature = "multiplayer")]
    multiplayer: multiplayer::State,
}

/// A public local-owner entry point. Roster administration stays on the trusted
/// host. Fresh one-use participant codes are printed only to that host terminal.
pub async fn serve<C>(
    roster: Roster,
    members: Vec<ParticipantRuntime>,
    address: SocketAddr,
    stop: C,
) -> Result<()>
where
    C: Future<Output = ()> + Send + 'static,
{
    roster
        .validate()
        .map_err(|_| Error::Config("invalid participant directory".into()))?;
    if !address.ip().is_loopback() {
        return Err(Error::Denied(
            "team service is numeric-loopback only; no public/proxy hosting",
        ));
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    let codes = (0..members.len())
        .map(|_| random_token())
        .collect::<Result<Vec<_>>>()?;
    let (router, team) = application(roster, members, address, &codes)?;
    eprintln!("aw team: http://{address}");
    for (entry, code) in team.0.entries.iter().zip(codes.iter()) {
        eprintln!(
            "Participant {}: {}",
            entry.user.id,
            entry.app.0.session.actor.as_str()
        );
        eprintln!(
            "Pairing code for {} (one use, 5 minutes): {}",
            entry.user.id, code
        );
    }
    eprintln!("Invite-only private conversations by default. Give each code only to its intended user; use separate browser profiles. Shared prompt collaboration is explicit and permissioned; no tools/shell/file views are granted.");
    let watchdog_team = team.clone();
    let watchdog = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = watchdog_team.0.stop.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            for entry in &watchdog_team.0.entries {
                let expired = {
                    let auth = lock(&entry.app.0.auth);
                    auth.cookie_hash.is_some() && !auth_alive_at(&auth)
                };
                if expired {
                    cancel_member(entry);
                }
            }
        }
    });
    let shutdown_team = team.clone();
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            stop.await;
            shutdown(&shutdown_team);
        })
        .await;
    shutdown(&team);
    watchdog.abort();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let busy = team
            .0
            .entries
            .iter()
            .any(|e| lock(&e.app.0.session.state).snapshot.pending.is_some());
        if !busy || Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    result?;
    Ok(())
}
fn cancel_member(entry: &Entry) {
    lock(&entry.app.0.auth).cookie_hash = None;
    if let Some(token) = &lock(&entry.app.0.session.state).cancel {
        token.cancel();
    }
    let _ = entry.app.0.session.events.send(WireEvent {
        sequence: 0,
        kind: "auth_expired",
        data: serde_json::json!({}),
    });
}
fn shutdown(team: &TeamState) {
    team.0.stop.cancel();
    for entry in &team.0.entries {
        entry.app.0.shutdown.cancel();
        cancel_member(entry);
        // Grants are process-owned. Revocation prevents later model rounds.
        if let Ok(accounts) = entry
            .app
            .0
            .session
            .accounts
            .visible_accounts(&entry.app.0.session.actor)
        {
            for account in accounts {
                let _ = entry.app.0.session.accounts.revoke(&account.id);
            }
        }
    }
}
fn application(
    roster: Roster,
    members: Vec<ParticipantRuntime>,
    address: SocketAddr,
    codes: &[String],
) -> Result<(Router, TeamState)> {
    roster
        .validate()
        .map_err(|_| Error::Config("invalid participant directory".into()))?;
    if members.len() != roster.participants().len()
        || codes.len() != members.len()
        || codes
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != codes.len()
    {
        return Err(Error::Denied(
            "every configured participant requires exactly one unique bootstrap",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut entries = Vec::new();
    for (member, code) in members.into_iter().zip(codes) {
        let user = roster
            .participant(&member.participant_id)
            .map_err(|_| Error::Denied("runtime principal absent from roster"))?
            .clone();
        let actor = roster
            .principal(&user.id)
            .map_err(|_| Error::Denied("invalid principal binding"))?;
        if !ids.insert(user.id.clone())
            || actor != member.agent.actor().as_str()
            || member.persistence.actor != *member.agent.actor()
            || member
                .persistence
                .store
                .session(
                    &member.persistence.actor,
                    member.persistence.handle.session_id(),
                )?
                .owner
                != member.persistence.actor
            || !member.agent.tool_specs().is_empty()
        {
            return Err(Error::Denied(
                "team participant requires its own actor/history and no tool grants",
            ));
        }
        let (router, app) = application_scoped(
            member.agent,
            Some(member.persistence),
            super::ApplicationScope {
                workspace_options: None,
                address,
                pairing_code: code.to_owned(),
                local_owner_uid: None,
                owned_sessions_only: true,
                authority: super::Authority::Browser,
                local_settings: None,
            },
        )?;
        *lock(&app.0.context_recipients) = roster
            .participants()
            .iter()
            .filter(|p| p.id != user.id)
            .map(|p| {
                roster
                    .principal(&p.id)
                    .map(|id| (id, p.display_name.clone()))
            })
            .collect::<std::result::Result<std::collections::BTreeMap<_, _>, _>>()
            .map_err(|_| Error::Denied("invalid recipient directory"))?;
        entries.push(Entry { user, app, router });
    }
    let team = TeamState(Arc::new(TeamInner {
        realm: roster.realm().into(),
        entries,
        pairing_failures: Mutex::new(Vec::new()),
        requests: Arc::new(Semaphore::new(MAX_GATEWAY_INFLIGHT)),
        stop: CancellationToken::new(),
        #[cfg(feature = "multiplayer")]
        multiplayer: multiplayer::State::default(),
    }));
    let router = Router::new().fallback(dispatch).with_state(team.clone());
    Ok((router, team))
}
fn response_policy(mut response: Response) -> Response {
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}
async fn dispatch(State(team): State<TeamState>, request: Request) -> Response {
    response_policy(match route(&team, request).await {
        Ok(response) => response,
        Err(e) => e.into_response(),
    })
}
fn authenticated_entry(team: &TeamState, headers: &HeaderMap) -> Option<Entry> {
    // Scan all bounded entries. Tokens have fresh 256-bit entropy; do not use an
    // untrusted username/actor/header/URL to choose a provider/session controller.
    let matches = team
        .0
        .entries
        .iter()
        .filter(|e| authenticated(&e.app, headers))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Some((*matches[0]).clone())
    } else {
        None
    }
}
async fn route(team: &TeamState, request: Request) -> ApiResult<Response> {
    if team.0.stop.is_cancelled() {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "team owner stopped",
        ));
    }
    let common = &team.0.entries[0].app;
    check_http_metadata(common, &request)?;
    let _permit = team
        .0
        .requests
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "owner request limit"))?;
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    if path == "/api/pair" && method == Method::POST {
        if authenticated_entry(team, request.headers()).is_some() {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "lock this browser identity before signing in as someone else",
            ));
        }
        let now = Instant::now();
        {
            let mut failures = lock(&team.0.pairing_failures);
            failures.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
            if failures.len() >= 10 {
                return Err(ApiError(
                    StatusCode::TOO_MANY_REQUESTS,
                    "too many sign-in attempts; wait one minute",
                ));
            }
            // Reserve each attempt before reading its body. Successful requests
            // also count; a single shared budget prevents per-user enumeration.
            failures.push(now);
        }
        let bytes = tokio::time::timeout(
            Duration::from_secs(5),
            to_bytes(request.into_body(), MAX_PAIR_BYTES),
        )
        .await
        .map_err(|_| ApiError(StatusCode::REQUEST_TIMEOUT, "sign-in body timeout"))?
        .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE, "sign-in body too large"))?;
        let command: PairCommand = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid sign-in request"))?;
        if !valid_token(&command.code) {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "invite invalid, used or expired",
            ));
        }
        let digest = hash(command.code.as_bytes());
        let candidate = team
            .0
            .entries
            .iter()
            .filter(|entry| {
                let auth = lock(&entry.app.0.auth);
                let same = bool::from(auth.pairing_hash.ct_eq(&digest));
                same && !auth.used && now < auth.pairing_expires
            })
            .cloned()
            .collect::<Vec<_>>();
        let entry = candidate.into_iter().next().ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "invite invalid, used or expired",
        ))?;
        if team.0.stop.is_cancelled() {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "team owner stopped"));
        }
        return pair(State(entry.app), Ok(Json(command))).await;
    }
    if path == "/healthz" && (method == Method::GET || method == Method::HEAD) {
        return Ok(Json(serde_json::json!({"service":"aw","phase":38,"mode":"invite-only-team","version":env!("CARGO_PKG_VERSION")})).into_response());
    }
    let public = matches!(
        path.as_str(),
        "/" | "/app.js"
            | "/chunk-code-surface.js"
            | "/chunk-multiplayer.js"
            | "/abird-round-trip-mark.svg"
            | "/model.js"
            | "/style.css"
            | "/workspace.js"
            | "/workspace-sidebars.js"
            | "/settings.js"
            | "/workspace-model.js"
            | "/workspace.css"
    );
    if public && (method == Method::GET || method == Method::HEAD) {
        return forward(team.0.entries[0].router.clone(), request).await;
    }
    let entry = authenticated_entry(team, request.headers()).ok_or(ApiError(
        StatusCode::UNAUTHORIZED,
        "sign in with your own invite",
    ))?;
    if path == "/api/team/me" && method == Method::GET {
        let peer_sharing = cfg!(feature = "multiplayer");
        let session_scope = if peer_sharing {
            "owned+explicit-shared"
        } else {
            "owned-only"
        };
        return Ok(Json(serde_json::json!({"mode":"invite-only-team","realm":team.0.realm,
            "participant":{"id":entry.user.id,"display_name":entry.user.display_name,"principal":entry.app.0.session.actor},
            "session_scope":session_scope,"tools":false,"surfaces":false,"peer_sharing":peer_sharing})).into_response());
    }
    let app = entry.app.clone();
    #[cfg(feature = "multiplayer")]
    if multiplayer::is_route(&path, &method) {
        let response = multiplayer::route(team, &entry, &path, request).await?;
        if !auth_alive(&app) || team.0.stop.is_cancelled() {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "identity revoked before collaboration response",
            ));
        }
        return Ok(response);
    }
    #[cfg(feature = "multiplayer")]
    multiplayer::guard_forward(&entry, &path, &method)?;
    let response = forward(entry.router, request).await?;
    if path != "/api/logout" && (!auth_alive(&app) || team.0.stop.is_cancelled()) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "identity revoked before response; inspect existing receipt after reauthentication",
        ));
    }
    Ok(response)
}
async fn forward(router: Router, request: Request) -> ApiResult<Response> {
    match tokio::time::timeout(Duration::from_secs(30), router.oneshot(request)).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(never)) => match never {},
        Err(_) => Err(ApiError(
            StatusCode::REQUEST_TIMEOUT,
            "request deadline; inspect receipt before retry",
        )),
    }
}

#[cfg(test)]
mod tests;
