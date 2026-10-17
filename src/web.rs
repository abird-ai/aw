//! Phase 3: paired loopback owner with optional durable session/branch state.
//! Authorized opaque account/model IDs may be selected. Tools require host grants.
//! No uploads, remote bind,
//! static filesystem serving, cookies in URLs, or implicit replay. Credentials/grants never restore.
use aw_agent_core::{
    AccountDescriptor, Agent, AgentEvent, CancellationToken, Error, InvocationIdentity,
    PrincipalId, ProviderAccounts, Result, ToolRecord, Turn,
};
use aw_store::SessionStore;
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::sse::Event,
    response::{Html, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::{stream, FutureExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    future::Future,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    sync::{broadcast, OwnedSemaphorePermit, Semaphore},
    time::Instant,
};

use crate::{
    config,
    context::ExecutionContext,
    sessions::{self, PersistentSession},
    LocalToolOptions,
};
mod agent_view;
mod context_flow;
#[cfg(target_os = "linux")]
mod native;
#[cfg(all(feature = "team", target_os = "linux"))]
pub mod team;
#[cfg(feature = "surfaces")]
mod workspace;
#[cfg(feature = "surfaces")]
pub use aw_surfaces::Options as WorkspaceOptions;
#[cfg(not(feature = "surfaces"))]
#[derive(Clone, Debug, Default)]
pub struct WorkspaceOptions;

#[cfg(not(feature = "surfaces"))]
mod workspace {
    use super::*;

    pub(super) async fn capabilities() -> ApiResult<Json<serde_json::Value>> {
        Ok(Json(serde_json::json!({
            "enabled": false,
            "hint": "this build omits human workspace surfaces"
        })))
    }

    pub(super) async fn action(
        State(app): State<AppState>,
        _payload: std::result::Result<
            Json<serde_json::Value>,
            axum::extract::rejection::JsonRejection,
        >,
    ) -> ApiResult<Json<serde_json::Value>> {
        if app.0.owned_sessions_only {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "workspace services are not granted to shared participants",
            ));
        }
        Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "this build omits human workspace surfaces",
        ))
    }

    pub(super) async fn script() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            include_str!("../web/workspace.js"),
        )
    }

    pub(super) async fn model() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            include_str!("../web/workspace-model.js"),
        )
    }

    pub(super) async fn styles() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            include_str!("../web/workspace.css"),
        )
    }
}

const PAIR_TTL: Duration = Duration::from_secs(300);
const SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);
const MAX_RECEIPTS: usize = 256;
const MAX_EVENT_CLIENTS: usize = 8;
const API_BODY_LIMIT: usize = aw_agent_core::limits::MAX_PROMPT_BYTES + 4096;
const CSP:&str="default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'";
const STYLE_NONCE_PLACEHOLDER: &str = "__AW_STYLE_NONCE__";

#[derive(Clone)]
struct HtmlStyleNonce(String);

#[derive(Clone)]
struct AppState(Arc<AppInner>, Authority);
#[derive(Clone, Copy)]
enum Authority {
    Browser,
    Local(u32),
    LocalWeb,
}
struct AppInner {
    local_owner_uid: Option<u32>,
    owned_sessions_only: bool,
    authority: String,
    origin: String,
    cookie_name: String,
    auth: Mutex<Auth>,
    session: Arc<Session>,
    shutdown: CancellationToken,
    event_slots: Arc<Semaphore>,
    branch_read_slots: Arc<Semaphore>,
    #[cfg(feature = "surfaces")]
    surfaces: Option<Arc<aw_surfaces::SurfaceHost>>,
    #[cfg(feature = "surfaces")]
    surface_slots: Arc<Semaphore>,
    csp: String,
    context_job: Mutex<Option<context_flow::Job>>,
    context_recipients: Mutex<BTreeMap<String, String>>,
    local_settings: Option<Mutex<LocalSettingsState>>,
    chatgpt_setup: Mutex<Option<ChatGptSetupState>>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceSummary {
    #[serde(default)]
    pub context: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Clone)]
pub struct LocalWebOptions {
    pub config_path: Option<PathBuf>,
    pub file_config: config::FileConfig,
    pub profiles: BTreeMap<String, config::Profile>,
    pub settings: config::Settings,
    pub context: ExecutionContext,
    pub tools: LocalToolOptions,
    pub state_dir: Option<PathBuf>,
    /// Existing owner Store shared with provider custody and later Session creation.
    pub store: Option<aw_store::SessionStore>,
    pub title: Option<String>,
    pub domain_selector: Option<String>,
    pub workspace_selector: Option<String>,
    pub resources: ResourceSummary,
}

#[derive(Clone, Debug, Serialize)]
struct ChatGptSetupModel {
    slug: String,
    display_name: String,
}

#[derive(Clone, Debug, Serialize)]
struct ChatGptSetupState {
    operation_id: String,
    profile_name: String,
    host_id: String,
    connector_id: String,
    status: String,
    authorization_url: Option<String>,
    models: Vec<ChatGptSetupModel>,
    error: Option<String>,
    #[serde(skip)]
    cancel: CancellationToken,
}

struct LocalSettingsState {
    config_path: Option<PathBuf>,
    file_config: config::FileConfig,
    profiles: BTreeMap<String, config::Profile>,
    settings: config::Settings,
    context: ExecutionContext,
    tools: LocalToolOptions,
    state_dir: Option<PathBuf>,
    title: Option<String>,
    domain_selector: Option<String>,
    workspace_selector: Option<String>,
}
impl From<LocalWebOptions> for LocalSettingsState {
    fn from(value: LocalWebOptions) -> Self {
        Self {
            config_path: value.config_path,
            file_config: value.file_config,
            profiles: value.profiles,
            settings: value.settings,
            context: value.context,
            tools: value.tools,
            state_dir: value.state_dir,
            title: value.title,
            domain_selector: value.domain_selector,
            workspace_selector: value.workspace_selector,
        }
    }
}

struct Auth {
    pairing_hash: [u8; 32],
    pairing_expires: Instant,
    used: bool,
    cookie_hash: Option<[u8; 32]>,
    session_expires: Option<Instant>,
    failures: Vec<Instant>,
}
struct Session {
    agent: tokio::sync::Mutex<Agent>,
    persistence: Mutex<Option<PersistentSession>>,
    store: Option<aw_store::SessionStore>,
    admission: tokio::sync::Mutex<()>,
    accounts: ProviderAccounts,
    actor: PrincipalId,
    state: Mutex<SessionState>,
    events: broadcast::Sender<WireEvent>,
}
struct SessionState {
    snapshot: Snapshot,
    next_run: u64,
    cancel: Option<CancellationToken>,
    receipts: BTreeMap<String, ([u8; 32], Receipt)>,
    completions: BTreeMap<u64, serde_json::Value>,
}
#[derive(Clone, Debug, Serialize)]
struct SettingsProfileView {
    name: String,
    profile: config::Profile,
    builtin: bool,
    is_default: bool,
}
#[derive(Clone, Debug, Serialize)]
struct SettingsView {
    profiles: Vec<SettingsProfileView>,
    default_profile: Option<String>,
    config_path: Option<String>,
    ui: config::UiConfig,
    local_single_user: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsProfileCommand {
    name: String,
    kind: config::ProviderKind,
    base_url: String,
    default_model: Option<String>,
    #[serde(default)]
    models: Vec<String>,
    api_key_env: Option<String>,
    #[serde(default)]
    allow_insecure_http: bool,
    #[serde(default)]
    make_default: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsNameCommand {
    name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatGptSetupCommand {
    operation_id: String,
    #[serde(default)]
    model: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsUiCommand {
    #[serde(default)]
    status_lines: Option<Vec<config::StatusLineConfig>>,
    #[serde(default)]
    status_line: Option<Vec<config::StatusItem>>,
    tui_mode: config::TuiMode,
    theme: config::ThemeMode,
    editor_padding_x: u8,
    output_padding: u8,
    #[serde(default)]
    workspace_editor_padding_x: Option<u8>,
    #[serde(default)]
    workspace_output_padding: Option<u8>,
    autocomplete_max_visible: u8,
    show_performance: bool,
    show_ttft: bool,
    show_waterfall: bool,
    show_thinking: bool,
    show_tool_calls: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountChoice {
    pub id: String,
    pub allowed_models: Vec<String>,
    pub account_kind: String,
    pub payer: aw_agent_core::BillingOwner,
    pub credential_custody: aw_agent_core::CredentialCustody,
    pub inference_executor: String,
}
impl From<AccountDescriptor> for AccountChoice {
    fn from(a: AccountDescriptor) -> Self {
        Self {
            id: a.id,
            allowed_models: a.allowed_models.into_iter().collect(),
            account_kind: match a.owner {
                aw_agent_core::AccountOwner::User(_) => "personal",
                aw_agent_core::AccountOwner::Server(_) => "shared-server",
            }
            .into(),
            payer: a.payer,
            credential_custody: a.credential_custody,
            inference_executor: a.inference_executor,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub instance: String,
    pub sequence: u64,
    pub revision: u64,
    pub model: String,
    pub profile: String,
    pub turns: Vec<Turn>,
    pub pending: Option<Pending>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub identity: Option<InvocationIdentity>,
    #[serde(default)]
    pub accounts: Vec<AccountChoice>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub tool_host: String,
    #[serde(default)]
    pub needs_reconciliation: bool,
    #[serde(default)]
    pub saved: Option<SavedSelection>,
    #[serde(default)]
    pub context_checkpoint: bool,
    #[serde(default)]
    pub context_blocks: Vec<aw_agent_core::ContextBlock>,
    #[serde(default)]
    pub resources: ResourceSummary,
    #[serde(default)]
    pub recovery_hints: Vec<aw_store::RecoveryHint>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSelection {
    pub session_id: String,
    pub branch_id: String,
    pub branch_name: String,
    pub head: String,
    pub branch_revision: u64,
    pub title: String,
    pub imported: bool,
}
fn saved_selection(p: &PersistentSession) -> Result<SavedSelection> {
    let b = p.handle.branch()?;
    let s = p.store.session(&p.actor, p.handle.session_id())?;
    Ok(SavedSelection {
        session_id: s.id,
        branch_id: b.id,
        branch_name: b.name,
        head: b.head,
        branch_revision: b.revision,
        title: s.title,
        imported: s.imported,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pending {
    pub run_id: u64,
    pub prompt: String,
    pub answer: String,
    #[serde(default)]
    pub tool_records: Vec<ToolRecord>,
    #[serde(default)]
    pub identity: Option<InvocationIdentity>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Receipt {
    run_id: u64,
    revision: u64,
    #[serde(default)]
    state: String,
}
impl From<aw_store::WebReceipt> for Receipt {
    fn from(r: aw_store::WebReceipt) -> Self {
        Self {
            run_id: r.run_id,
            revision: r.revision,
            state: r.state,
        }
    }
}
#[derive(Clone)]
struct WireEvent {
    sequence: u64,
    kind: &'static str,
    data: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairCommand {
    code: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptCommand {
    request_id: String,
    expected_revision: u64,
    prompt: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    branch_id: Option<String>,
    #[serde(default)]
    retry_operation_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcknowledgeCommand {
    expected_revision: u64,
    confirmed_inspected: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelCommand {
    run_id: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetCommand {
    expected_revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenCommand {
    expected_revision: u64,
    session_id: String,
    branch: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForkCommand {
    expected_revision: u64,
    name: String,
    #[serde(default)]
    at_node: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointCommand {
    expected_revision: u64,
    summary: String,
}

#[derive(Debug)]
struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
type ApiResult<T> = std::result::Result<T, ApiError>;
fn api_error(error: Error) -> ApiError {
    match error {
        Error::Denied(_) => ApiError(
            StatusCode::FORBIDDEN,
            "session/action not permitted by current context",
        ),
        Error::Conflict(_) => ApiError(
            StatusCode::CONFLICT,
            "session, target or receipt changed; inspect and refresh",
        ),
        Error::Limit(_) => ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "bounded store/view limit reached; use CLI checkpoint/export",
        ),
        Error::Config(_) | Error::Protocol(_) => {
            ApiError(StatusCode::BAD_REQUEST, "invalid session operation")
        }
        _ => ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "durable state unavailable; do not replay; inspect/restart owner",
        ),
    }
}
async fn store_work<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "store worker failed"))?
        .map_err(api_error)
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(feature = "surfaces")]
async fn shutdown_surfaces(app: &AppState) {
    if let Some(host) = app.0.surfaces.clone() {
        let _ = tokio::task::spawn_blocking(move || host.shutdown()).await;
    }
}

#[cfg(not(feature = "surfaces"))]
async fn shutdown_surfaces(_app: &AppState) {}

fn hash(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}
fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Config("OS randomness unavailable".into()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}
fn safe_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next().unwrap_or("").trim() == "application/json")
}

/// Explicit ephemeral owner for SDK tests/consumers. The normal CLI uses
/// serve_with_state unless --no-state was explicitly selected.
pub async fn serve<C>(agent: Agent, listen: SocketAddr, shutdown: C) -> Result<()>
where
    C: Future<Output = ()> + Send + 'static,
{
    serve_with_state(agent, None, listen, shutdown).await
}
pub async fn serve_with_state<C>(
    agent: Agent,
    persistence: Option<PersistentSession>,
    listen: SocketAddr,
    shutdown: C,
) -> Result<()>
where
    C: Future<Output = ()> + Send + 'static,
{
    serve_with_workspace(agent, persistence, None, listen, shutdown).await
}
/// Same runtime with optional human workspace grants. No service is enabled by opening a tab alone.
pub async fn serve_with_workspace<C>(
    agent: Agent,
    persistence: Option<PersistentSession>,
    options: Option<WorkspaceOptions>,
    listen: SocketAddr,
    shutdown: C,
) -> Result<()>
where
    C: Future<Output = ()> + Send + 'static,
{
    if !listen.ip().is_loopback() {
        return Err(Error::Config(
            "web binds only to a numeric loopback address".into(),
        ));
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let address = listener.local_addr()?;
    let pairing_code = random_token()?;
    let durable = persistence.is_some();
    let (router, app) =
        application_with_workspace(agent, persistence, options, address, &pairing_code)?;
    eprintln!("aw web: http://{address}");
    eprintln!("Pairing code (one use, expires in 5 minutes): {pairing_code}");
    eprintln!("Private loopback owner · {}. Current accounts/tools only; browser provider keys are never collected.",if durable{"durable history"}else{"explicit memory-only mode"});
    // Expired browser authority cannot leave a shell control surface live.
    let expiry_app = app.clone();
    let watchdog = tokio::spawn(async move {
        loop {
            tokio::select! { _=expiry_app.0.shutdown.cancelled()=>break, _=tokio::time::sleep(Duration::from_secs(5))=>{} }
            let expired = {
                let auth = lock(&expiry_app.0.auth);
                auth.used && !auth_alive_at(&auth)
            };
            if expired {
                shutdown_surfaces(&expiry_app).await;
                break;
            }
        }
    });
    let stop = app.clone();
    let served = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.await;
            stop.0.shutdown.cancel();
            if let Some(token) = &lock(&stop.0.session.state).cancel {
                token.cancel();
            }
        })
        .await;
    app.0.shutdown.cancel();
    watchdog.abort();
    shutdown_surfaces(&app).await;
    // Give cancellation a bounded chance to settle its journal. A subsequent
    // owner recovers anything still marked running; never replay it here.
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let busy = lock(&app.0.session.state).snapshot.pending.is_some();
        if !busy || Instant::now() >= until {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    served?;
    Ok(())
}

/// Same-user numeric-loopback web UI. This intentionally has no pairing step:
/// it is the local user's own control surface, protected by loopback binding,
/// strict Host/Origin/Sec-Fetch checks and browser same-origin policy. Team and
/// `aw agent --serve-web` browser grants continue to use explicit pairing.
pub async fn serve_single_user<C>(
    agent: Agent,
    persistence: Option<PersistentSession>,
    options: Option<WorkspaceOptions>,
    local: LocalWebOptions,
    listen: SocketAddr,
    shutdown: C,
) -> Result<()>
where
    C: Future<Output = ()> + Send + 'static,
{
    if !listen.ip().is_loopback() {
        return Err(Error::Config(
            "web binds only to a numeric loopback address".into(),
        ));
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let address = listener.local_addr()?;
    let code = random_token()?; // retained only for shared router construction; never shown/used.
    let uid = local.context.effective_uid;
    let (router, app) = application_scoped(
        agent,
        persistence,
        ApplicationScope {
            workspace_options: options,
            address,
            pairing_code: code,
            local_owner_uid: uid,
            owned_sessions_only: false,
            authority: Authority::LocalWeb,
            local_settings: Some(local),
        },
    )?;
    eprintln!("aw web: http://{address}");
    eprintln!("Local single-user mode · no pairing · configure providers/models in Settings.");
    let stop = app.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.await;
            stop.0.shutdown.cancel();
            if let Some(token) = &lock(&stop.0.session.state).cancel {
                token.cancel();
            }
        })
        .await?;
    app.0.shutdown.cancel();
    shutdown_surfaces(&app).await;
    Ok(())
}

/// One Unix-principal owner for native clients and an explicitly enabled paired
/// browser. `aw web` retains its separate browser principal and old defaults.
/// This is local Linux IPC, not remote hosting, OIDC or a personal-key bridge.
#[cfg(target_os = "linux")]
pub async fn serve_local_owner<C>(
    agent: Agent,
    persistence: Option<PersistentSession>,
    options: Option<WorkspaceOptions>,
    socket: &std::path::Path,
    http: Option<SocketAddr>,
    shutdown: C,
) -> Result<()>
where
    C: Future<Output = ()> + Send + 'static,
{
    let uid = aw_client::local::effective_uid().map_err(|e| Error::Config(e.to_string()))?;
    if agent.actor().as_str() != format!("unix:{uid}") {
        return Err(Error::Denied(
            "local server actor does not match effective Unix account",
        ));
    }
    let bound =
        aw_client::local::BoundSocket::bind(socket).map_err(|e| Error::Config(e.to_string()))?;
    let listener = if let Some(address) = http {
        if !address.ip().is_loopback() {
            return Err(Error::Config(
                "shared owner HTTP is numeric-loopback only".into(),
            ));
        }
        Some(tokio::net::TcpListener::bind(address).await?)
    } else {
        None
    };
    let address = match &listener {
        Some(l) => l.local_addr()?,
        None => SocketAddr::from(([127, 0, 0, 1], 0)),
    };
    let code = random_token()?;
    let (router, app) =
        application_configured(agent, persistence, options, address, &code, Some(uid))?;
    eprintln!("aw owner: {} · unix:{uid}", socket.display());
    eprintln!("Attach uses explicitly configured OWNER accounts/tools; no client credentials are uploaded or inferred.");
    if listener.is_some() {
        eprintln!("aw web: http://{address}");
        eprintln!("Pairing code (one use, expires in 5 minutes): {code}");
        eprintln!("EXPLICIT GRANT: pairing this browser uses the same Unix-owner principal, saved-session access and configured account authority.");
    }
    let native_app = app.clone();
    let native = native::serve(native_app, bound);
    let stop = app.0.shutdown.clone();
    let browser = async move {
        if let Some(listener) = listener {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move { stop.cancelled().await })
                .await
        } else {
            std::future::pending::<std::io::Result<()>>().await
        }
    };
    tokio::pin!(native);
    tokio::pin!(browser);
    tokio::pin!(shutdown);
    // Browser-only terminal grants still expire independently of Unix attach.
    let expiry_app = app.clone();
    let watchdog = tokio::spawn(async move {
        loop {
            tokio::select! {_ = expiry_app.0.shutdown.cancelled()=>break,_ = tokio::time::sleep(Duration::from_secs(5))=>{}}
            let expired = {
                let auth = lock(&expiry_app.0.auth);
                auth.used && !auth_alive_at(&auth)
            };
            if expired {
                shutdown_surfaces(&expiry_app).await;
                break;
            }
        }
    });
    let outcome = tokio::select! {
        _ = &mut shutdown=>Ok(()),
        result = &mut native=>result,
        result = &mut browser=>result.map_err(Error::Io),
    };
    app.0.shutdown.cancel();
    watchdog.abort();
    if let Some(token) = &lock(&app.0.session.state).cancel {
        token.cancel();
    }
    shutdown_surfaces(&app).await;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if lock(&app.0.session.state).snapshot.pending.is_none() || Instant::now() >= until {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Dropping listeners/connections does not replay pending external effects.
    // The next owner recovers an unfinished journal using the existing rules.
    outcome
}

#[cfg(test)]
fn application(
    agent: Agent,
    address: SocketAddr,
    pairing_code: &str,
) -> Result<(Router, AppState)> {
    application_with_state(agent, None, address, pairing_code)
}
#[cfg(test)]
fn application_with_state(
    agent: Agent,
    persistence: Option<PersistentSession>,
    address: SocketAddr,
    pairing_code: &str,
) -> Result<(Router, AppState)> {
    application_with_workspace(agent, persistence, None, address, pairing_code)
}
struct ApplicationScope {
    workspace_options: Option<WorkspaceOptions>,
    address: SocketAddr,
    pairing_code: String,
    local_owner_uid: Option<u32>,
    owned_sessions_only: bool,
    authority: Authority,
    local_settings: Option<LocalWebOptions>,
}

fn application_with_workspace(
    agent: Agent,
    persistence: Option<PersistentSession>,
    workspace_options: Option<WorkspaceOptions>,
    address: SocketAddr,
    pairing_code: &str,
) -> Result<(Router, AppState)> {
    application_configured(
        agent,
        persistence,
        workspace_options,
        address,
        pairing_code,
        None,
    )
}
fn application_configured(
    agent: Agent,
    persistence: Option<PersistentSession>,
    workspace_options: Option<WorkspaceOptions>,
    address: SocketAddr,
    pairing_code: &str,
    local_owner_uid: Option<u32>,
) -> Result<(Router, AppState)> {
    application_scoped(
        agent,
        persistence,
        ApplicationScope {
            workspace_options,
            address,
            pairing_code: pairing_code.to_owned(),
            local_owner_uid,
            owned_sessions_only: false,
            authority: Authority::Browser,
            local_settings: None,
        },
    )
}
fn application_scoped(
    mut agent: Agent,
    persistence: Option<PersistentSession>,
    scope: ApplicationScope,
) -> Result<(Router, AppState)> {
    let store = persistence.as_ref().map(|p| p.store.clone()).or_else(|| {
        scope
            .local_settings
            .as_ref()
            .and_then(|local| local.store.clone())
    });
    if let Some(local) = &scope.local_settings {
        match (local.state_dir.as_deref(), store.as_ref()) {
            (Some(configured), Some(store))
                if store.directory() == Some(configured.canonicalize()?.as_path()) => {}
            (Some(_), _) => {
                return Err(Error::Denied(
                    "local web requires its existing configured owner Store",
                ))
            }
            (None, Some(store)) if store.directory().is_some() => {
                return Err(Error::Denied("local web State is disabled"))
            }
            _ => {}
        }
        if let (Some(provided), Some(retained)) = (&local.store, &store) {
            if provided.store_id()? != retained.store_id()? {
                return Err(Error::Denied(
                    "local web Store authority differs from current Session",
                ));
            }
        }
    }
    let resources = scope
        .local_settings
        .as_ref()
        .map(|settings| settings.resources.clone())
        .unwrap_or_default();
    let ApplicationScope {
        workspace_options,
        address,
        pairing_code,
        local_owner_uid,
        owned_sessions_only,
        authority,
        local_settings,
    } = scope;
    if let Some(uid) = local_owner_uid {
        if agent.actor().as_str() != format!("unix:{uid}") {
            return Err(Error::Denied(
                "local owner requires its verified Unix principal",
            ));
        }
    }
    if let Some(p) = &persistence {
        p.apply(&mut agent)?;
    }
    let saved = persistence.as_ref().map(saved_selection).transpose()?;
    let turns = agent.turns().to_vec();
    let needs_reconciliation = agent.needs_reconciliation();
    let context_checkpoint = agent.history_state().summary.is_some();
    let context_blocks = agent.history_state().context_blocks;
    if !address.ip().is_loopback() || !valid_token(&pairing_code) {
        return Err(Error::Config("invalid local web bootstrap".into()));
    }
    let actor = agent.actor().clone();
    let accounts = agent.provider_accounts();
    let identity = agent.default_identity().ok().or_else(|| {
        let settings = local_settings.as_ref()?;
        let name = settings
            .file_config
            .default_profile
            .as_ref()
            .unwrap_or(&settings.settings.profile_name);
        let profile = settings.profiles.get(name)?;
        let model = profile
            .model
            .clone()
            .or_else(|| profile.effective_models().into_iter().next())?;
        accounts
            .select(&actor, name, &model)
            .ok()
            .map(|selection| selection.identity().clone())
    });
    let visible = accounts
        .visible_accounts(&actor)?
        .into_iter()
        .map(AccountChoice::from)
        .collect();
    let tools = agent.tool_specs().into_iter().map(|t| t.name).collect();
    let tool_host = agent.tool_execution_site().to_owned();
    #[cfg(feature = "surfaces")]
    let surfaces = workspace_options
        .map(|mut o| {
            o.actor = actor.as_str().to_owned();
            aw_surfaces::SurfaceHost::new(o)
                .map(Arc::new)
                .map_err(|e| Error::Config(e.to_string()))
        })
        .transpose()?;
    #[cfg(not(feature = "surfaces"))]
    let _ = workspace_options;

    let csp = CSP.to_string();
    #[cfg(feature = "surfaces")]
    let mut csp = csp;
    #[cfg(feature = "surfaces")]
    if let Some(host) = &surfaces {
        csp.push_str("; style-src-attr 'unsafe-inline'; frame-src");
        if host.sites().is_empty() {
            csp.push_str(" 'none'");
        } else {
            for site in host.sites() {
                csp.push(' ');
                csp.push_str(&site.origin());
            }
        }
    }
    let (event_tx, _) = broadcast::channel(128);
    let state = AppState(
        Arc::new(AppInner {
            local_owner_uid,
            owned_sessions_only,
            authority: address.to_string(),
            origin: format!("http://{address}"),
            cookie_name: format!("aw_session_{}", address.port()),
            auth: Mutex::new(Auth {
                pairing_hash: hash(pairing_code.as_bytes()),
                pairing_expires: Instant::now() + PAIR_TTL,
                used: false,
                cookie_hash: None,
                session_expires: None,
                failures: Vec::new(),
            }),
            session: Arc::new(Session {
                agent: tokio::sync::Mutex::new(agent),
                persistence: Mutex::new(persistence),
                store,
                admission: tokio::sync::Mutex::new(()),
                accounts,
                actor,
                events: event_tx,
                state: Mutex::new(SessionState {
                    snapshot: Snapshot {
                        instance: random_token()?,
                        sequence: 0,
                        revision: 0,
                        model: identity
                            .as_ref()
                            .map(|i| i.model.clone())
                            .unwrap_or_default(),
                        profile: identity
                            .as_ref()
                            .map(|i| i.account_id.clone())
                            .unwrap_or_default(),
                        turns,
                        pending: None,
                        last_error: None,
                        identity,
                        accounts: visible,
                        tools,
                        tool_host,
                        needs_reconciliation,
                        saved,
                        context_checkpoint,
                        context_blocks,
                        resources,
                        recovery_hints: Vec::new(),
                    },
                    next_run: 1,
                    cancel: None,
                    receipts: BTreeMap::new(),
                    completions: BTreeMap::new(),
                }),
            }),
            shutdown: CancellationToken::new(),
            event_slots: Arc::new(Semaphore::new(MAX_EVENT_CLIENTS)),
            branch_read_slots: Arc::new(Semaphore::new(4)),
            #[cfg(feature = "surfaces")]
            surfaces,
            #[cfg(feature = "surfaces")]
            surface_slots: Arc::new(Semaphore::new(4)),
            csp,
            context_job: Mutex::new(None),
            context_recipients: Mutex::new(BTreeMap::new()),
            local_settings: local_settings.map(|value| Mutex::new(value.into())),
            chatgpt_setup: Mutex::new(None),
        }),
        authority,
    );
    let router=Router::new()
        .route("/",get(index))
        .route("/theme-bootstrap.js", get(theme_bootstrap_script))
        .route("/theme-bootstrap.css", get(theme_bootstrap_styles))
        .route("/app.js",get(script))
        .route("/chunk-code-surface.js", get(code_surface_chunk))
        .route("/chunk-multiplayer.js", get(multiplayer_chunk))
        .route("/abird-round-trip-mark.svg", get(brand_mark))
        .route("/model.js",get(model_script))
        .route("/settings.js",get(settings_script))
        .route("/context-flow.js",get(context_flow::script))
        .route("/api/context/recipients",get(context_flow::recipients))
        .route("/api/context/recovery",get(context_flow::recovery))
        .route("/api/context/action",post(context_flow::action))
        .route("/api/context/cancel",post(context_flow::cancel_job))
        .route("/style.css",get(styles))
        .route("/workspace.js",get(workspace::script))
        .route("/workspace-sidebars.js",get(workspace_sidebars_script))
        .route("/workspace-model.js",get(workspace::model))
        .route("/workspace.css",get(workspace::styles))
        .route("/api/workspace",get(workspace::capabilities))
        .route("/api/workspace/action",post(workspace::action))
        .route("/api/settings",get(settings_view))
        .route("/api/settings/profile",post(settings_profile))
        .route("/api/settings/profile/remove",post(settings_profile_remove))
        .route("/api/settings/default",post(settings_default))
        .route("/api/settings/ui",post(settings_ui))
        .route("/api/settings/chatgpt/start", post(settings_chatgpt_start))
        .route("/api/settings/chatgpt/status", get(settings_chatgpt_status))
        .route("/api/settings/chatgpt/cancel", post(settings_chatgpt_cancel))
        .route("/api/settings/chatgpt/finish", post(settings_chatgpt_finish))
        .route("/healthz",get(||async {Json(serde_json::json!({"service":"aw","phase":7,"version":env!("CARGO_PKG_VERSION")}))}))
        .route("/api/pair",post(pair))
        .route("/api/session",get(snapshot))
        .route("/api/accounts",get(self::accounts))
        .route("/api/sessions",get(session_list))
        .route("/api/agent-view",post(agent_view::read))
        .route("/api/branches",get(branch_list))
        .route("/api/tree",get(tree_view))
        .route("/api/usage",get(stored_usage))
        .route("/api/export",get(export_view))
        .route("/api/session/open",post(open_session))
        .route("/api/fork",post(fork_session))
        .route("/api/checkpoint",post(checkpoint))
        .route("/api/acknowledge",post(acknowledge))
        .route("/api/events",get(events))
        .route("/api/prompts",post(prompt))
        .route("/api/cancel",post(cancel))
        .route("/api/reset",post(reset))
        .route("/api/logout",post(logout))
        .layer(DefaultBodyLimit::max(API_BODY_LIMIT))
        .layer(middleware::from_fn_with_state(state.clone(),boundary))
        .with_state(state.clone());
    Ok((router, state))
}
async fn index() -> ApiResult<Response> {
    let template = include_str!("../web/dist/index.html");
    if !template.contains(STYLE_NONCE_PLACEHOLDER) {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "browser style nonce template unavailable",
        ));
    }
    let nonce = random_token().map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "secure randomness unavailable",
        )
    })?;
    let mut response = Html(template.replace(STYLE_NONCE_PLACEHOLDER, &nonce)).into_response();
    response.extensions_mut().insert(HtmlStyleNonce(nonce));
    Ok(response)
}
async fn theme_bootstrap_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/dist/theme-bootstrap.js"),
    )
}
async fn theme_bootstrap_styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/dist/theme-bootstrap.css"),
    )
}
async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/dist/app.js"),
    )
}
async fn code_surface_chunk() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/dist/chunk-code-surface.js"),
    )
}
async fn multiplayer_chunk() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/dist/chunk-multiplayer.js"),
    )
}

async fn brand_mark() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/svg+xml")],
        include_str!("../web/dist/abird-round-trip-mark.svg"),
    )
}
async fn model_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/model.js"),
    )
}
async fn settings_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/settings.js"),
    )
}
async fn workspace_sidebars_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/workspace-sidebars.js"),
    )
}
async fn styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/dist/style.css"),
    )
}

async fn boundary(State(app): State<AppState>, request: Request, next: Next) -> Response {
    let denied = check_boundary(&app, &request);
    let mut policy = app.0.csp.clone();
    let mut response = match denied {
        Ok(()) => next.run(request).await,
        Err(e) => e.into_response(),
    };
    if let Some(HtmlStyleNonce(nonce)) = response.extensions_mut().remove::<HtmlStyleNonce>() {
        policy = policy.replacen(
            "style-src 'self'",
            &format!("style-src 'self' 'nonce-{nonce}'"),
            1,
        );
    }
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&policy).unwrap_or_else(|_| HeaderValue::from_static(CSP)),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}
fn check_http_metadata(app: &AppState, request: &Request) -> ApiResult<()> {
    let h = request.headers();
    if h.get_all(header::HOST).iter().count() != 1
        || h.get(header::HOST).and_then(|v| v.to_str().ok()) != Some(app.0.authority.as_str())
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "unrecognized host"));
    }
    if let Some(a) = request.uri().authority() {
        if a.as_str() != app.0.authority {
            return Err(ApiError(StatusCode::FORBIDDEN, "unrecognized authority"));
        }
    }
    let origin = h.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if h.get_all(header::ORIGIN).iter().count() > 1
        || (h.contains_key(header::ORIGIN) && origin != Some(app.0.origin.as_str()))
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "origin denied"));
    }
    if h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return Err(ApiError(StatusCode::FORBIDDEN, "cross-site request denied"));
    }
    if request.uri().path().starts_with("/api/")
        && request.method() != Method::GET
        && (origin != Some(app.0.origin.as_str())
            || h.get_all(header::CONTENT_TYPE).iter().count() != 1
            || !safe_json(h))
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "same-origin JSON request required",
        ));
    }
    Ok(())
}
fn check_boundary(app: &AppState, request: &Request) -> ApiResult<()> {
    check_http_metadata(app, request)?;
    let path = request.uri().path();
    if path.starts_with("/api/")
        && path != "/api/pair"
        && matches!(app.1, Authority::Browser)
        && !authenticated(app, request.headers())
    {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "pair this browser first",
        ));
    }
    Ok(())
}
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut found = None;
    for header in headers.get_all(header::COOKIE).iter() {
        for part in header.to_str().ok()?.split(';') {
            let (k, v) = part.trim().split_once('=')?;
            if k == name {
                if found.is_some() {
                    return None;
                }
                found = Some(v);
            }
        }
    }
    found
}
fn authenticated(app: &AppState, headers: &HeaderMap) -> bool {
    if app.0.shutdown.is_cancelled() {
        return false;
    }
    let Some(token) = cookie_value(headers, &app.0.cookie_name) else {
        return false;
    };
    if !valid_token(token) {
        return false;
    }
    let auth = lock(&app.0.auth);
    auth.session_expires.is_some_and(|t| Instant::now() < t)
        && auth
            .cookie_hash
            .is_some_and(|h| bool::from(h.ct_eq(&hash(token.as_bytes()))))
}
fn auth_alive_at(auth: &Auth) -> bool {
    auth.cookie_hash.is_some() && auth.session_expires.is_some_and(|t| Instant::now() < t)
}
fn auth_alive(app: &AppState) -> bool {
    if app.0.shutdown.is_cancelled() {
        return false;
    }
    match app.1 {
        Authority::Browser => auth_alive_at(&lock(&app.0.auth)),
        Authority::Local(uid) => app.0.local_owner_uid == Some(uid),
        Authority::LocalWeb => true,
    }
}
async fn pair(
    State(app): State<AppState>,
    payload: std::result::Result<Json<PairCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Response> {
    if matches!(app.1, Authority::LocalWeb) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "local single-user web does not use pairing",
        ));
    }
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid pairing request"))?;
    let mut auth = lock(&app.0.auth);
    let now = Instant::now();
    auth.failures
        .retain(|t| now.duration_since(*t) < Duration::from_secs(60));
    if auth.failures.len() >= 10 {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many pairing attempts; wait one minute",
        ));
    }
    if auth.used
        || now >= auth.pairing_expires
        || !valid_token(&command.code)
        || !bool::from(auth.pairing_hash.ct_eq(&hash(command.code.as_bytes())))
    {
        auth.failures.push(now);
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "pairing code invalid, used or expired; restart owner for a new code",
        ));
    }
    let token = random_token().map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "secure randomness unavailable",
        )
    })?;
    auth.used = true;
    auth.cookie_hash = Some(hash(token.as_bytes()));
    auth.session_expires = Some(now + SESSION_TTL);
    let cookie = format!(
        "{}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        app.0.cookie_name,
        SESSION_TTL.as_secs()
    );
    let mut response = Json(serde_json::json!({"paired":true})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "cannot create browser session",
            )
        })?,
    );
    Ok(response)
}
fn local_settings_guard(
    app: &AppState,
) -> ApiResult<std::sync::MutexGuard<'_, LocalSettingsState>> {
    if !matches!(app.1, Authority::LocalWeb) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "settings are available only in local single-user web mode",
        ));
    }
    let settings = app.0.local_settings.as_ref().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "local settings are unavailable",
    ))?;
    settings
        .lock()
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "settings unavailable"))
}

fn publish_local_file_config(
    settings: &mut LocalSettingsState,
    next: config::FileConfig,
) -> ApiResult<()> {
    let committed = config::save_changes(
        settings.config_path.as_deref(),
        &settings.file_config,
        &next,
    )
    .map_err(api_error)?;
    settings.file_config = committed;
    settings.profiles = config::builtins();
    settings
        .profiles
        .extend(settings.file_config.profiles.clone());
    Ok(())
}

fn settings_projection(settings: &LocalSettingsState) -> SettingsView {
    let builtins = config::builtins();
    let mut profiles = settings
        .profiles
        .iter()
        .map(|(name, profile)| SettingsProfileView {
            name: name.clone(),
            profile: profile.clone(),
            builtin: builtins.contains_key(name)
                && !settings.file_config.profiles.contains_key(name),
            is_default: settings.file_config.default_profile.as_deref() == Some(name.as_str()),
        })
        .collect::<Vec<_>>();
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    SettingsView {
        profiles,
        default_profile: settings.file_config.default_profile.clone(),
        config_path: settings
            .config_path
            .clone()
            .or_else(config::default_config_path)
            .map(|path| path.display().to_string()),
        ui: settings.file_config.ui.clone(),
        local_single_user: true,
        notice: None,
    }
}

fn refresh_profile_projection(app: &AppState) -> ApiResult<()> {
    let (visible, identity) = {
        let settings = local_settings_guard(app)?;
        let visible = app
            .0
            .session
            .accounts
            .visible_accounts(&app.0.session.actor)
            .map_err(api_error)?;
        let selected = settings
            .file_config
            .default_profile
            .as_ref()
            .and_then(|name| settings.profiles.get(name).map(|profile| (name, profile)))
            .and_then(|(name, profile)| {
                profile.model.clone().and_then(|model| {
                    app.0
                        .session
                        .accounts
                        .select(&app.0.session.actor, name, &model)
                        .ok()
                        .map(|selection| selection.identity().clone())
                })
            });
        (visible, selected)
    };
    let mut state = lock(&app.0.session.state);
    state.snapshot.accounts = visible.into_iter().map(AccountChoice::from).collect();
    state.snapshot.profile = identity
        .as_ref()
        .map(|i| i.account_id.clone())
        .unwrap_or_default();
    state.snapshot.model = identity
        .as_ref()
        .map(|i| i.model.clone())
        .unwrap_or_default();
    state.snapshot.identity = identity;
    state.snapshot.revision = state.snapshot.revision.saturating_add(1);
    let revision = state.snapshot.revision;
    let event = wire(
        &mut state.snapshot,
        "changed",
        serde_json::json!({"revision":revision,"settings":true}),
    );
    let _ = app.0.session.events.send(event);
    Ok(())
}

fn settings_idle(app: &AppState) -> ApiResult<()> {
    if lock(&app.0.session.state).snapshot.pending.is_some()
        || lock(&app.0.context_job)
            .as_ref()
            .is_some_and(|job| job.result.is_none())
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "model/context work is active; wait for it to settle before changing settings",
        ));
    }
    Ok(())
}

fn chatgpt_setup_projection(app: &AppState) -> ApiResult<Option<ChatGptSetupState>> {
    if !matches!(app.1, Authority::LocalWeb) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "ChatGPT setup is available only in local single-user web mode",
        ));
    }
    Ok(lock(&app.0.chatgpt_setup).clone())
}

async fn settings_chatgpt_status(
    State(app): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "setup": chatgpt_setup_projection(&app)?
    })))
}

async fn settings_chatgpt_start(State(app): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    if !matches!(app.1, Authority::LocalWeb) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "ChatGPT setup is available only in local single-user web mode",
        ));
    }
    settings_idle(&app)?;

    #[cfg(not(feature = "oidc"))]
    {
        let _ = app;
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "this build disabled Sign in with ChatGPT",
        ));
    }

    #[cfg(feature = "oidc")]
    {
        let (_state_dir, actor, profile_name, host_id, referenced_connectors) = {
            let mut settings = local_settings_guard(&app)?;
            let state_dir = settings.state_dir.clone().ok_or(ApiError(
                StatusCode::BAD_REQUEST,
                "Sign in with ChatGPT requires durable local state",
            ))?;
            let actor = crate::sessions::actor(&settings.context, false).map_err(api_error)?;
            let base = "chatgpt";
            let mut name = base.to_owned();
            for suffix in 2..=99 {
                if !settings.profiles.contains_key(&name) {
                    break;
                }
                name = format!("{base}-{suffix}");
            }
            let generated = crate::oidc_flow::new_chatgpt_host_id().map_err(api_error)?;
            let (committed, host_id) = config::mutate(settings.config_path.as_deref(), |cfg| {
                if let Some(host_id) = cfg.chatgpt_host_id.clone() {
                    config::validate_oauth_host_id(&host_id)?;
                    Ok(host_id)
                } else {
                    cfg.chatgpt_host_id = Some(generated.clone());
                    Ok(generated.clone())
                }
            })
            .map_err(api_error)?;
            settings.file_config = committed;
            let configured_profiles = settings.file_config.profiles.clone();
            settings.profiles = config::builtins();
            settings.profiles.extend(configured_profiles);
            let referenced_connectors = settings
                .file_config
                .profiles
                .values()
                .filter_map(|profile| profile.oauth_connector.clone())
                .collect::<std::collections::BTreeSet<_>>();
            (state_dir, actor, name, host_id, referenced_connectors)
        };

        let previous_setup = { lock(&app.0.chatgpt_setup).clone() };
        if let Some(previous) = previous_setup {
            if matches!(previous.status.as_str(), "starting" | "waiting") {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "a ChatGPT sign-in is already pending",
                ));
            }
            if previous.status == "ready" {
                previous.cancel.cancel();
                if let Ok(store) = current_store(&app) {
                    let _ = crate::oidc_flow::chatgpt_plan_disconnect(
                        &store,
                        &actor,
                        &previous.connector_id,
                    )
                    .await;
                }
            }
        }

        let store = current_store(&app)?;
        crate::oidc_flow::chatgpt_plan_cleanup_unreferenced(&store, &actor, &referenced_connectors)
            .await
            .map_err(api_error)?;

        let operation = random_token().map_err(api_error)?;
        let operation_id = format!("chatgpt-{}", &operation[..16]);
        let connector_id = format!("chatgpt-{}", &operation[16..32]);
        let setup_cancel = CancellationToken::new();
        *lock(&app.0.chatgpt_setup) = Some(ChatGptSetupState {
            operation_id: operation_id.clone(),
            profile_name: profile_name.clone(),
            host_id: host_id.clone(),
            connector_id: connector_id.clone(),
            status: "starting".into(),
            authorization_url: None,
            models: Vec::new(),
            error: None,
            cancel: setup_cancel.clone(),
        });

        let task_app = app.clone();
        let task_operation = operation_id.clone();
        let task_connector = connector_id.clone();
        let (url_tx, url_rx) = tokio::sync::oneshot::channel::<String>();
        tokio::spawn(async move {
            let authorize = crate::oidc_flow::chatgpt_plan_authorize(
                &store,
                &actor,
                &task_connector,
                &host_id,
                SocketAddr::from(([127, 0, 0, 1], 0)),
                Duration::from_secs(300),
                move |url| {
                    let _ = url_tx.send(url.to_owned());
                },
            );
            let result = tokio::select! {
                biased;
                result = authorize => Some(result),
                _ = setup_cancel.cancelled() => None,
            };
            let Some(result) = result else {
                return;
            };

            match result {
                Ok(_) => {
                    if setup_cancel.is_cancelled() {
                        let _ = crate::oidc_flow::chatgpt_plan_disconnect(
                            &store,
                            &actor,
                            &task_connector,
                        )
                        .await;
                        return;
                    }
                    let model_result = tokio::select! {
                        biased;
                        result = crate::oidc_flow::chatgpt_plan_models(&store, &actor, &task_connector) => Some(result),
                        _ = setup_cancel.cancelled() => None,
                    };
                    let Some(model_result) = model_result else {
                        let _ = crate::oidc_flow::chatgpt_plan_disconnect(
                            &store,
                            &actor,
                            &task_connector,
                        )
                        .await;
                        return;
                    };
                    match model_result {
                        Ok(models) => {
                            if setup_cancel.is_cancelled() {
                                let _ = crate::oidc_flow::chatgpt_plan_disconnect(
                                    &store,
                                    &actor,
                                    &task_connector,
                                )
                                .await;
                                return;
                            }
                            {
                                let mut slot = lock(&task_app.0.chatgpt_setup);
                                if let Some(setup) = slot
                                    .as_mut()
                                    .filter(|setup| setup.operation_id == task_operation)
                                {
                                    setup.models = models
                                        .into_iter()
                                        .map(|model| ChatGptSetupModel {
                                            slug: model.slug,
                                            display_name: model.display_name,
                                        })
                                        .collect();
                                    setup.status = "ready".into();
                                    setup.error = None;
                                }
                            }
                            tokio::select! {
                                _ = tokio::time::sleep(Duration::from_secs(600)) => {}
                                _ = setup_cancel.cancelled() => return,
                            }
                            let should_expire = lock(&task_app.0.chatgpt_setup)
                                .as_ref()
                                .is_some_and(|setup| {
                                    setup.operation_id == task_operation && setup.status == "ready"
                                });
                            if should_expire {
                                let cleanup = crate::oidc_flow::chatgpt_plan_disconnect(
                                    &store,
                                    &actor,
                                    &task_connector,
                                )
                                .await;
                                let mut slot = lock(&task_app.0.chatgpt_setup);
                                if let Some(setup) = slot
                                    .as_mut()
                                    .filter(|setup| setup.operation_id == task_operation)
                                {
                                    setup.models.clear();
                                    setup.authorization_url = None;
                                    match cleanup {
                                        Ok(remote) => {
                                            setup.status = "expired".into();
                                            setup.error = (!remote).then(|| {
                                                "ChatGPT setup expired; local credentials were cleared, but remote revocation was not confirmed.".into()
                                            });
                                        }
                                        Err(error) => {
                                            setup.status = "failed".into();
                                            setup.error = Some(format!(
                                                "ChatGPT setup expired but local credential cleanup failed: {error}"
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            let _ = crate::oidc_flow::chatgpt_plan_disconnect(
                                &store,
                                &actor,
                                &task_connector,
                            )
                            .await;
                            let mut slot = lock(&task_app.0.chatgpt_setup);
                            if let Some(setup) = slot
                                .as_mut()
                                .filter(|setup| setup.operation_id == task_operation)
                            {
                                setup.status = "failed".into();
                                setup.error = Some(error.to_string());
                            }
                        }
                    }
                }
                Err(error) => {
                    let mut slot = lock(&task_app.0.chatgpt_setup);
                    if let Some(setup) = slot
                        .as_mut()
                        .filter(|setup| setup.operation_id == task_operation)
                    {
                        setup.status = "failed".into();
                        setup.error = Some(error.to_string());
                    }
                }
            }
        });

        let authorization_url = tokio::time::timeout(Duration::from_secs(5), url_rx)
            .await
            .map_err(|_| {
                ApiError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "ChatGPT sign-in did not initialize",
                )
            })?
            .map_err(|_| {
                ApiError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "ChatGPT sign-in failed before authorization",
                )
            })?;
        {
            let mut slot = lock(&app.0.chatgpt_setup);
            if let Some(setup) = slot
                .as_mut()
                .filter(|setup| setup.operation_id == operation_id)
            {
                setup.authorization_url = Some(authorization_url.clone());
                setup.status = "waiting".into();
            }
        }
        Ok(Json(serde_json::json!({
            "operation_id": operation_id,
            "authorization_url": authorization_url
        })))
    }
}

async fn settings_chatgpt_cancel(
    State(app): State<AppState>,
    payload: std::result::Result<
        Json<ChatGptSetupCommand>,
        axum::extract::rejection::JsonRejection,
    >,
) -> ApiResult<Json<serde_json::Value>> {
    if !matches!(app.1, Authority::LocalWeb) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "ChatGPT setup is available only in local single-user web mode",
        ));
    }
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid ChatGPT setup request"))?;
    let (setup, connector_exists) = {
        let mut slot = lock(&app.0.chatgpt_setup);
        let setup = slot
            .as_mut()
            .filter(|setup| setup.operation_id == command.operation_id)
            .ok_or(ApiError(StatusCode::CONFLICT, "ChatGPT setup is stale"))?;
        if setup.status == "configured" {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "configured ChatGPT accounts are managed as provider profiles",
            ));
        }
        let connector_exists = setup.status == "ready";
        setup.cancel.cancel();
        setup.status = "cancelled".into();
        setup.authorization_url = None;
        setup.models.clear();
        (setup.clone(), connector_exists)
    };

    #[cfg(feature = "oidc")]
    let remote_revocation_confirmed: Option<bool> = {
        let (state_dir, actor) = {
            let settings = local_settings_guard(&app)?;
            (
                settings.state_dir.clone(),
                crate::sessions::actor(&settings.context, false).map_err(api_error)?,
            )
        };
        if connector_exists {
            let _path = state_dir.ok_or(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ChatGPT setup lost its durable state directory",
            ))?;
            let store = current_store(&app)?;
            Some(
                crate::oidc_flow::chatgpt_plan_disconnect(&store, &actor, &setup.connector_id)
                    .await
                    .map_err(api_error)?,
            )
        } else {
            // If cancellation won before connector creation, the background
            // task observes the shared token. If OAuth completed at the same
            // boundary, that task disconnects before returning.
            None
        }
    };
    #[cfg(not(feature = "oidc"))]
    let remote_revocation_confirmed: Option<bool> = {
        let _ = (&setup, connector_exists);
        None
    };

    Ok(Json(serde_json::json!({
        "cancelled": true,
        "remote_revocation_confirmed": remote_revocation_confirmed
    })))
}

async fn settings_chatgpt_finish(
    State(app): State<AppState>,
    payload: std::result::Result<
        Json<ChatGptSetupCommand>,
        axum::extract::rejection::JsonRejection,
    >,
) -> ApiResult<Json<SettingsView>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid ChatGPT setup request"))?;
    let _admission = app.0.session.admission.lock().await;
    settings_idle(&app)?;

    #[cfg(not(feature = "oidc"))]
    {
        let _ = (&app, &command.operation_id, &command.model);
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "this build disabled Sign in with ChatGPT",
        ));
    }

    #[cfg(feature = "oidc")]
    {
        let model = command.model.ok_or(ApiError(
            StatusCode::BAD_REQUEST,
            "choose a ChatGPT model before finishing setup",
        ))?;
        let setup = lock(&app.0.chatgpt_setup)
            .clone()
            .filter(|setup| setup.operation_id == command.operation_id)
            .ok_or(ApiError(StatusCode::CONFLICT, "ChatGPT setup is stale"))?;
        if setup.status != "ready" {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "ChatGPT sign-in is not ready for model selection",
            ));
        }
        if !setup.models.iter().any(|entry| entry.slug == model) {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "selected ChatGPT model is not in the authorized catalog",
            ));
        }

        let (idle_timeout, profile, store) = {
            let mut settings = local_settings_guard(&app)?;
            let _path = settings.state_dir.clone().ok_or(ApiError(
                StatusCode::BAD_REQUEST,
                "Sign in with ChatGPT requires durable local state",
            ))?;
            let models = setup
                .models
                .iter()
                .map(|entry| entry.slug.clone())
                .collect::<Vec<_>>();
            let (next, profile) = config::apply_profile_update(
                &settings.file_config,
                config::ProfileUpdate {
                    name: setup.profile_name.clone(),
                    kind: config::ProviderKind::OpenaiResponses,
                    base_url: "https://api.openai.com/v1".into(),
                    default_model: Some(model),
                    models,
                    api_key_env: None,
                    oauth_connector: Some(setup.connector_id.clone()),
                    oauth_host_id: Some(setup.host_id.clone()),
                    allow_insecure_http: false,
                    token_field: config::TokenField::MaxTokens,
                    include_usage: true,
                    make_default: true,
                },
            )
            .map_err(api_error)?;
            publish_local_file_config(&mut settings, next)?;
            settings.settings.profile_name = setup.profile_name.clone();
            settings.settings.profile = profile.clone();
            (
                settings.settings.idle_timeout,
                profile,
                current_store(&app)?,
            )
        };

        {
            let agent = app.0.session.agent.lock().await;
            crate::register_oauth_profile(
                &agent,
                &setup.profile_name,
                &profile,
                idle_timeout,
                store,
            )
            .map_err(api_error)?;
        }
        refresh_profile_projection(&app)?;
        if let Some(current) = lock(&app.0.chatgpt_setup)
            .as_mut()
            .filter(|current| current.operation_id == command.operation_id)
        {
            current.cancel.cancel();
            current.status = "configured".into();
        }
        let settings = local_settings_guard(&app)?;
        Ok(Json(settings_projection(&settings)))
    }
}

async fn settings_view(State(app): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let settings = local_settings_guard(&app)?;
    Ok(Json(settings_projection(&settings)))
}

async fn settings_profile(
    State(app): State<AppState>,
    payload: std::result::Result<
        Json<SettingsProfileCommand>,
        axum::extract::rejection::JsonRejection,
    >,
) -> ApiResult<Json<SettingsView>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid settings profile"))?;
    let _admission = app.0.session.admission.lock().await;
    settings_idle(&app)?;
    let name = command.name.clone();
    {
        let settings = local_settings_guard(&app)?;
        if settings
            .file_config
            .profiles
            .get(&name)
            .is_some_and(|profile| profile.oauth_connector.is_some())
        {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "OAuth-backed ChatGPT profiles must be managed through ChatGPT account controls",
            ));
        }
    }
    let (idle_timeout, profile) = {
        let mut settings = local_settings_guard(&app)?;
        let (next, profile) = config::apply_profile_update(
            &settings.file_config,
            config::ProfileUpdate {
                name: command.name,
                kind: command.kind,
                base_url: command.base_url,
                default_model: command.default_model,
                models: command.models,
                api_key_env: command.api_key_env,
                oauth_connector: None,
                oauth_host_id: None,
                allow_insecure_http: command.allow_insecure_http,
                token_field: config::TokenField::MaxTokens,
                include_usage: false,
                make_default: command.make_default,
            },
        )
        .map_err(api_error)?;
        publish_local_file_config(&mut settings, next)?;
        (settings.settings.idle_timeout, profile)
    };
    {
        let agent = app.0.session.agent.lock().await;
        crate::register_local_profile(&agent, &name, &profile, idle_timeout).map_err(api_error)?;
    }
    refresh_profile_projection(&app)?;
    let settings = local_settings_guard(&app)?;
    Ok(Json(settings_projection(&settings)))
}

async fn settings_profile_remove(
    State(app): State<AppState>,
    payload: std::result::Result<
        Json<SettingsNameCommand>,
        axum::extract::rejection::JsonRejection,
    >,
) -> ApiResult<Json<SettingsView>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid settings request"))?;
    let _admission = app.0.session.admission.lock().await;
    settings_idle(&app)?;

    let oauth_connector = {
        let settings = local_settings_guard(&app)?;
        settings
            .file_config
            .profiles
            .get(&command.name)
            .ok_or(ApiError(
                StatusCode::BAD_REQUEST,
                "only configured profile overrides can be removed",
            ))?
            .oauth_connector
            .clone()
    };
    #[cfg(feature = "oidc")]
    let mut removal_notice = None;
    #[cfg(not(feature = "oidc"))]
    let removal_notice: Option<String> = None;
    #[cfg(feature = "oidc")]
    if let Some(connector_id) = oauth_connector {
        let (_state_dir, actor) = {
            let settings = local_settings_guard(&app)?;
            (
                settings.state_dir.clone().ok_or(ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "OAuth-backed profile lost its durable state directory",
                ))?,
                crate::sessions::actor(&settings.context, false).map_err(api_error)?,
            )
        };
        let store = current_store(&app)?;
        let remote_confirmed =
            crate::oidc_flow::chatgpt_plan_disconnect(&store, &actor, &connector_id)
                .await
                .map_err(api_error)?;
        if !remote_confirmed {
            removal_notice = Some(
                "ChatGPT was disconnected locally, but remote OAuth revocation was not confirmed. Review app access in ChatGPT settings."
                    .into(),
            );
        }
    }
    #[cfg(not(feature = "oidc"))]
    if oauth_connector.is_some() {
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "this build cannot disconnect an OAuth-backed ChatGPT profile",
        ));
    }

    {
        let mut settings = local_settings_guard(&app)?;
        let mut next = settings.file_config.clone();
        if next.profiles.remove(&command.name).is_none() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "only configured profile overrides can be removed",
            ));
        }
        if next.default_profile.as_deref() == Some(command.name.as_str()) {
            next.default_profile = None;
        }
        publish_local_file_config(&mut settings, next)?;
    }
    app.0
        .session
        .accounts
        .revoke(&command.name)
        .map_err(api_error)?;
    refresh_profile_projection(&app)?;
    let settings = local_settings_guard(&app)?;
    let mut view = settings_projection(&settings);
    view.notice = removal_notice;
    Ok(Json(view))
}

async fn settings_default(
    State(app): State<AppState>,
    payload: std::result::Result<
        Json<SettingsNameCommand>,
        axum::extract::rejection::JsonRejection,
    >,
) -> ApiResult<Json<SettingsView>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid settings request"))?;
    let _admission = app.0.session.admission.lock().await;
    settings_idle(&app)?;
    {
        let mut settings = local_settings_guard(&app)?;
        let profile = settings
            .profiles
            .get(&command.name)
            .ok_or(ApiError(StatusCode::BAD_REQUEST, "unknown profile"))?;
        if profile.model.is_none() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "profile has no explicit default model",
            ));
        }
        let mut next = settings.file_config.clone();
        next.default_profile = Some(command.name);
        publish_local_file_config(&mut settings, next)?;
    }
    refresh_profile_projection(&app)?;
    let settings = local_settings_guard(&app)?;
    Ok(Json(settings_projection(&settings)))
}

async fn settings_ui(
    State(app): State<AppState>,
    payload: std::result::Result<Json<SettingsUiCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<SettingsView>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid UI settings"))?;
    let _admission = app.0.session.admission.lock().await;
    settings_idle(&app)?;
    if command.status_lines.is_some() && command.status_line.is_some() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "use status_lines or legacy status_line, not both",
        ));
    }
    let current_ui = local_settings_guard(&app)?.file_config.ui.clone();
    let current_status_lines = current_ui.status_lines.clone();
    let status_lines = command
        .status_lines
        .or_else(|| {
            command.status_line.map(|items| {
                vec![config::StatusLineConfig {
                    region: config::StatusRegion::BelowComposer,
                    items,
                    right_items: Vec::new(),
                }]
            })
        })
        .unwrap_or(current_status_lines);
    let ui = config::UiConfig {
        status_lines,
        tui_mode: command.tui_mode,
        theme: command.theme,
        theme_accent: current_ui.theme_accent.clone(),
        editor_padding_x: command.editor_padding_x,
        output_padding: command.output_padding,
        workspace_editor_padding_x: command
            .workspace_editor_padding_x
            .unwrap_or(current_ui.workspace_editor_padding_x),
        workspace_output_padding: command
            .workspace_output_padding
            .unwrap_or(current_ui.workspace_output_padding),
        autocomplete_max_visible: command.autocomplete_max_visible,
        show_performance: command.show_performance,
        show_ttft: command.show_ttft,
        show_waterfall: command.show_waterfall,
        show_thinking: command.show_thinking,
        show_tool_calls: command.show_tool_calls,
        show_event_notices: current_ui.show_event_notices,
    };
    ui.validate().map_err(api_error)?;
    let mut settings = local_settings_guard(&app)?;
    let mut next = settings.file_config.clone();
    next.ui = ui;
    publish_local_file_config(&mut settings, next)?;
    Ok(Json(settings_projection(&settings)))
}

async fn snapshot(State(app): State<AppState>) -> ApiResult<Json<Snapshot>> {
    context_flow::visible(&app).await?;
    let persistent = lock(&app.0.session.persistence).clone();
    let hints = if let Some(p) = persistent {
        let session = p.handle.session_id().to_owned();
        store_work(move || p.store.recovery_hints(&p.actor, Some(&session))).await?
    } else {
        Vec::new()
    };
    let mut snapshot = lock(&app.0.session.state).snapshot.clone();
    snapshot.recovery_hints = hints;
    snapshot.accounts = app
        .0
        .session
        .accounts
        .visible_accounts(&app.0.session.actor)
        .unwrap_or_default()
        .into_iter()
        .map(AccountChoice::from)
        .collect();
    Ok(Json(snapshot))
}
async fn accounts(State(app): State<AppState>) -> ApiResult<Json<Vec<AccountChoice>>> {
    Ok(Json(
        app.0
            .session
            .accounts
            .visible_accounts(&app.0.session.actor)
            .map_err(|_| ApiError(StatusCode::FORBIDDEN, "accounts unavailable"))?
            .into_iter()
            .map(AccountChoice::from)
            .collect(),
    ))
}

fn wire(snapshot: &mut Snapshot, kind: &'static str, data: serde_json::Value) -> WireEvent {
    snapshot.sequence += 1;
    WireEvent {
        sequence: snapshot.sequence,
        kind,
        data,
    }
}
impl Session {
    fn delta(&self, run_id: u64, text: &str) {
        let mut state = lock(&self.state);
        if let Some(p) = state
            .snapshot
            .pending
            .as_mut()
            .filter(|p| p.run_id == run_id)
        {
            p.answer.push_str(text);
        } else {
            return;
        }
        let event = wire(
            &mut state.snapshot,
            "delta",
            serde_json::json!({"run_id":run_id,"text":text}),
        );
        let _ = self.events.send(event);
    }
    fn tool_event(&self, run_id: u64, record: &ToolRecord) {
        let mut state = lock(&self.state);
        if let Some(p) = state
            .snapshot
            .pending
            .as_mut()
            .filter(|p| p.run_id == run_id)
        {
            if let Some(prior) = p
                .tool_records
                .iter_mut()
                .find(|r| r.call.id == record.call.id)
            {
                *prior = record.clone();
            } else {
                p.tool_records.push(record.clone());
            }
        } else {
            return;
        }
        let event = wire(
            &mut state.snapshot,
            "changed",
            serde_json::json!({"tool_changed":true}),
        );
        let _ = self.events.send(event);
    }
    fn settle(&self, run_id: u64, result: Result<Turn>, saved: Option<SavedSelection>) {
        let mut state = lock(&self.state);
        if state
            .snapshot
            .pending
            .as_ref()
            .is_none_or(|p| p.run_id != run_id)
        {
            return;
        }
        // Small memory-only receipts retain outcome metadata across reset, not
        // duplicate full transcripts. Durable result lookup comes from SQLite.
        if state.receipts.values().any(|(_, r)| r.run_id == run_id) {
            let completion = match &result {
                Ok(turn) => {
                    serde_json::json!({"turn_id":turn.id,"outcome":turn.outcome,"error":turn.error.as_deref().map(|e|e.chars().take(2048).collect::<String>()),"needs_reconciliation":turn.needs_reconciliation})
                }
                Err(_) => {
                    serde_json::json!({"outcome":"failed","error":"owner preparation or settlement failed; inspect state","needs_reconciliation":true})
                }
            };
            state.completions.insert(run_id, completion);
        }
        match result {
            Ok(turn) => {
                state.snapshot.needs_reconciliation |= turn.needs_reconciliation;
                state.snapshot.last_error = turn.error.clone();
                state.snapshot.turns.push(turn);
            }
            Err(error) => {
                state.snapshot.last_error = Some(error.to_string());
                if matches!(error, Error::Storage(_))
                    || state
                        .snapshot
                        .pending
                        .as_ref()
                        .is_some_and(|p| !p.tool_records.is_empty())
                {
                    state.snapshot.needs_reconciliation = true;
                }
            }
        }
        if saved.is_some() {
            state.snapshot.saved = saved;
        }
        for (_, r) in state.receipts.values_mut() {
            if r.run_id == run_id {
                r.state = "settled".into();
            }
        }
        state.snapshot.pending = None;
        state.cancel = None;
        state.snapshot.revision += 1;
        state.snapshot.sequence += 1;
        let event = WireEvent {
            sequence: state.snapshot.sequence,
            kind: "changed",
            data: serde_json::json!({"revision":state.snapshot.revision}),
        };
        let _ = self.events.send(event);
    }
}
async fn ensure_local_persistence(
    app: &AppState,
    account: &str,
    model: &str,
    prompt: &str,
) -> ApiResult<Option<PersistentSession>> {
    if let Some(existing) = lock(&app.0.session.persistence).clone() {
        return Ok(Some(existing));
    }
    let Some(settings_mutex) = app.0.local_settings.as_ref() else {
        return Ok(None);
    };
    let setup = {
        let settings = settings_mutex
            .lock()
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "settings unavailable"))?;
        let Some(state_dir) = settings.state_dir.clone() else {
            return Ok(None);
        };
        let mut profile = settings.profiles.get(account).cloned().ok_or(ApiError(
            StatusCode::FORBIDDEN,
            "selected profile no longer exists",
        ))?;
        if !profile.effective_models().contains(model) {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "selected model is no longer allowed",
            ));
        }
        profile.model = Some(model.to_owned());
        let mut selected = settings.settings.clone();
        selected.profile_name = account.to_owned();
        selected.profile = profile;
        (
            state_dir,
            selected,
            settings.context.clone(),
            settings.tools.clone(),
            settings.title.clone(),
            settings.domain_selector.clone(),
            settings.workspace_selector.clone(),
        )
    };
    let actor = app.0.session.actor.clone();
    let title = setup
        .4
        .clone()
        .unwrap_or_else(|| prompt.trim().chars().take(100).collect::<String>());
    let store = current_store(app)?;
    let created = store_work(move || {
        let binding = sessions::binding(&setup.1, &setup.2, &setup.3)?;
        PersistentSession::create_in_workspace(
            store,
            actor,
            binding,
            if title.is_empty() {
                "New agent"
            } else {
                &title
            },
            setup.5.as_deref(),
            setup.6.as_deref(),
            None,
        )
    })
    .await?;
    {
        let mut agent = app.0.session.agent.lock().await;
        created.apply(&mut agent).map_err(api_error)?;
    }
    let saved = saved_selection(&created).map_err(api_error)?;
    *lock(&app.0.session.persistence) = Some(created.clone());
    lock(&app.0.session.state).snapshot.saved = Some(saved);
    Ok(Some(created))
}

async fn prompt(
    State(app): State<AppState>,
    payload: std::result::Result<Json<PromptCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<(StatusCode, Json<Receipt>)> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid prompt request"))?;
    if command.request_id.is_empty()
        || command.request_id.len() > 64
        || !command
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid request_id"));
    }
    if command.prompt.trim().is_empty()
        || command.prompt.len() > aw_agent_core::limits::MAX_PROMPT_BYTES
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "prompt outside byte limit",
        ));
    }
    if command.account_id.is_some() != command.model.is_some() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "select both allowed account and model, or neither",
        ));
    }
    let _admission = app.0.session.admission.lock().await;
    if !auth_alive(&app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
    }
    let default = lock(&app.0.session.state).snapshot.identity.clone();
    let account = command
        .account_id
        .clone()
        .or_else(|| default.as_ref().map(|value| value.account_id.clone()))
        .ok_or(ApiError(
            StatusCode::FORBIDDEN,
            "configure and select an account/model first",
        ))?;
    let model = command
        .model
        .clone()
        .or_else(|| default.as_ref().map(|value| value.model.clone()))
        .ok_or(ApiError(
            StatusCode::FORBIDDEN,
            "configure and select an account/model first",
        ))?;
    if account.len() > 128 || model.len() > 256 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid account/model label",
        ));
    }
    let fingerprint = hash(
        serde_json::to_string(&(
            command.expected_revision,
            &command.prompt,
            &account,
            &model,
            app.0.session.actor.as_str(),
            &command.session_id,
            &command.branch_id,
            &command.retry_operation_id,
        ))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid prompt"))?
        .as_bytes(),
    );
    let digest = fingerprint
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    // The retained owner Store resolves accepted IDs before Session creation,
    // account selection or revision checks, including after an unsaved restart.
    let receipt_key = format!("{}:{}", app.0.session.actor.as_str(), command.request_id);
    if let Some(store) = app.0.session.store.clone() {
        let actor = app.0.session.actor.clone();
        let request = command.request_id.clone();
        let d = digest.clone();
        if let Some(r) = store_work(move || store.lookup_web(&actor, &request, &d)).await? {
            return Ok((StatusCode::ACCEPTED, Json(r.into())));
        }
    } else if let Some((prior, receipt)) = lock(&app.0.session.state).receipts.get(&receipt_key) {
        if prior == &fingerprint {
            return Ok((StatusCode::ACCEPTED, Json(receipt.clone())));
        }
        return Err(ApiError(
            StatusCode::CONFLICT,
            "request ID reused for different input",
        ));
    }
    if lock(&app.0.context_job)
        .as_ref()
        .is_some_and(|j| j.result.is_none())
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "summary generation in progress; no prompt queued",
        ));
    }
    let selected = app
        .0
        .session
        .accounts
        .select(&app.0.session.actor, &account, &model)
        .map_err(|_| {
            ApiError(
                StatusCode::FORBIDDEN,
                "selected account/model not permitted; no fallback",
            )
        })?;
    let originally_unsaved = lock(&app.0.session.persistence).is_none()
        && app
            .0
            .local_settings
            .as_ref()
            .is_some_and(|settings| lock(settings).state_dir.is_some());
    if originally_unsaved {
        // Only the admitted original unsaved target may use the new Session.
        idle_revision(&app, command.expected_revision)?;
        if lock(&app.0.session.state).snapshot.needs_reconciliation
            || command.session_id.is_some()
            || command.branch_id.is_some()
            || command.retry_operation_id.is_some()
        {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "unsaved target changed; refresh",
            ));
        }
    }
    let persistence = ensure_local_persistence(&app, &account, &model, &command.prompt).await?;
    let created_here = originally_unsaved && persistence.is_some();
    let next_revision = {
        let state = lock(&app.0.session.state);
        if state.snapshot.pending.is_some()
            || state.snapshot.needs_reconciliation
            || state.snapshot.revision != command.expected_revision
        {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "busy, needs inspection, or stale revision; refresh",
            ));
        }
        if let Some(saved) = &state.snapshot.saved {
            if !created_here
                && (command.session_id.as_deref() != Some(saved.session_id.as_str())
                    || command.branch_id.as_deref() != Some(saved.branch_id.as_str()))
            {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "exact saved session and branch target required",
                ));
            }
        } else if command.session_id.is_some() || command.branch_id.is_some() {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "memory owner has no saved target",
            ));
        }
        if persistence.is_none() && state.receipts.len() >= MAX_RECEIPTS {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "ephemeral request limit reached",
            ));
        }
        state.snapshot.revision + 1
    };
    let receipt = if let Some(p) = &persistence {
        let p = p.clone();
        let request = command.request_id.clone();
        let session = command.session_id.clone().unwrap_or_default();
        let branch = command.branch_id.clone().unwrap_or_default();
        Receipt::from(
            store_work(move || {
                let (session, branch) = if created_here {
                    (p.handle.session_id().to_owned(), p.handle.branch()?.id)
                } else {
                    (session, branch)
                };
                p.store.reserve_web(
                    &p.actor,
                    &request,
                    &digest,
                    &session,
                    &branch,
                    next_revision,
                )
            })
            .await?,
        )
    } else {
        Receipt {
            run_id: lock(&app.0.session.state).next_run,
            revision: next_revision,
            state: "accepted".into(),
        }
    };
    if !auth_alive(&app) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "session expired after admission; receipt retained, no replay",
        ));
    }
    let token = CancellationToken::new();
    {
        let mut state = lock(&app.0.session.state);
        state.next_run = state.next_run.max(receipt.run_id.saturating_add(1));
        state.snapshot.revision = next_revision;
        state.snapshot.last_error = None;
        state.snapshot.pending = Some(Pending {
            run_id: receipt.run_id,
            prompt: command.prompt.clone(),
            answer: String::new(),
            tool_records: Vec::new(),
            identity: Some(selected.identity().clone()),
        });
        state.cancel = Some(token.clone());
        if persistence.is_none() {
            state
                .receipts
                .insert(receipt_key, (fingerprint, receipt.clone()));
        }
        let event = wire(
            &mut state.snapshot,
            "changed",
            serde_json::json!({"revision":next_revision}),
        );
        let _ = app.0.session.events.send(event);
    }
    let session = app.0.session.clone();
    let run_id = receipt.run_id;
    tokio::spawn(async move {
        let run = async {
            let mut agent = session.agent.lock().await;
            if let Some(p) = &persistence {
                p.handle.bind_web_receipt(run_id)?;
                if let Some(operation) = &command.retry_operation_id {
                    let op = p.store.authorize_retry(&p.actor, operation)?;
                    if op.prompt != command.prompt {
                        return Err(Error::Conflict("retry prompt differs from durable original; choose a new operation instead"));
                    }
                    p.handle.bind_retry_operation(operation.clone())?;
                }
            }
            agent
                .prompt_as(&selected, command.prompt, token.cancelled(), |event| {
                    match event {
                        AgentEvent::Delta { text, .. } => session.delta(run_id, text),
                        AgentEvent::ToolStarted { record, .. }
                        | AgentEvent::ToolFinished { record, .. } => {
                            session.tool_event(run_id, record)
                        }
                        _ => {}
                    }
                    Ok(())
                })
                .await
        };
        let mut result = std::panic::AssertUnwindSafe(run)
            .catch_unwind()
            .await
            .unwrap_or(Err(Error::Storage(
                "native execution panicked; restart and inspect durable receipt",
            )));
        // Preflight can reject before journal.begin consumes this local hint.
        // Clearing it never deletes the durable deduplication receipt.
        if let Some(p) = &persistence {
            if p.handle.clear_web_receipt(run_id).is_err() {
                result = Err(Error::Storage("request binding unavailable; restart owner"));
            }
            if p.handle.clear_retry_operation().is_err() {
                result = Err(Error::Storage("retry binding unavailable; restart owner"));
            }
        }
        let gate = session.agent.lock().await.needs_reconciliation();
        if gate {
            lock(&session.state).snapshot.needs_reconciliation = true;
        }
        let saved = if let Some(p) = persistence {
            let out = tokio::task::spawn_blocking(move || {
                p.store.settle_web(&p.actor, run_id)?;
                saved_selection(&p)
            })
            .await;
            match out {
                Ok(Ok(s)) => Some(s),
                _ => {
                    result = Err(Error::Storage(
                        "receipt settlement failed; restart/recover before continuing",
                    ));
                    None
                }
            }
        } else {
            None
        };
        session.settle(run_id, result, saved);
    });
    Ok((StatusCode::ACCEPTED, Json(receipt)))
}
async fn cancel(
    State(app): State<AppState>,
    payload: std::result::Result<Json<CancelCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid cancellation request"))?;
    let state = lock(&app.0.session.state);
    if let Some(p) = &state.snapshot.pending {
        if p.run_id != command.run_id {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "that run is not the active run",
            ));
        }
        if let Some(token) = &state.cancel {
            token.cancel();
        }
        Ok(Json(
            serde_json::json!({"cancellation_requested":true,"run_id":command.run_id}),
        ))
    } else {
        Ok(Json(
            serde_json::json!({"cancellation_requested":false,"run_id":command.run_id,"state":"already_settled"}),
        ))
    }
}
fn current_persistence(app: &AppState) -> ApiResult<PersistentSession> {
    lock(&app.0.session.persistence).clone().ok_or(ApiError(
        StatusCode::CONFLICT,
        "this owner explicitly disabled persistence",
    ))
}
fn current_store(app: &AppState) -> ApiResult<SessionStore> {
    app.0.session.store.clone().ok_or(ApiError(
        StatusCode::FORBIDDEN,
        "this owner has no SQLite State authority",
    ))
}
fn idle_revision(app: &AppState, revision: u64) -> ApiResult<()> {
    if lock(&app.0.context_job)
        .as_ref()
        .is_some_and(|j| j.result.is_none())
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "a summary is still running; inspect or cancel it",
        ));
    }
    let s = lock(&app.0.session.state);
    if s.snapshot.pending.is_some() || s.snapshot.revision != revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "session busy or changed; refresh",
        ));
    }
    Ok(())
}
fn update_projection(
    app: &AppState,
    agent: &Agent,
    saved: Option<SavedSelection>,
    preserve_gate: bool,
) -> Snapshot {
    let mut state = lock(&app.0.session.state);
    let prior = state.snapshot.needs_reconciliation;
    state.snapshot.turns = agent.turns().to_vec();
    state.snapshot.saved = saved;
    state.snapshot.context_checkpoint = agent.history_state().summary.is_some();
    state.snapshot.context_blocks = agent.history_state().context_blocks;
    state.snapshot.needs_reconciliation = agent.needs_reconciliation() || (preserve_gate && prior);
    state.snapshot.pending = None;
    state.snapshot.last_error = None;
    state.snapshot.revision += 1;
    let revision = state.snapshot.revision;
    let event = wire(
        &mut state.snapshot,
        "changed",
        serde_json::json!({"revision":revision}),
    );
    let _ = app.0.session.events.send(event);
    state.snapshot.clone()
}
enum ViewChange {
    New,
    Open(String, String),
    Fork(String, Option<String>),
    Checkpoint(String),
}
async fn change_view(
    app: &AppState,
    revision: u64,
    change: ViewChange,
) -> ApiResult<Json<Snapshot>> {
    change_view_with_membership(app, revision, change, false).await
}

async fn change_view_with_membership(
    app: &AppState,
    revision: u64,
    change: ViewChange,
    allow_explicit_member: bool,
) -> ApiResult<Json<Snapshot>> {
    let _admission = app.0.session.admission.lock().await;
    if !auth_alive(app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
    }
    idle_revision(app, revision)?;
    let mut agent = app
        .0
        .session
        .agent
        .try_lock()
        .map_err(|_| ApiError(StatusCode::CONFLICT, "run still settling"))?;
    let p = lock(&app.0.session.persistence).clone();
    let store = current_store(app)?;
    let actor = app.0.session.actor.clone();
    let setup = if p.is_none() {
        if !matches!(&change, ViewChange::Open(_, _)) {
            return Err(ApiError(StatusCode::CONFLICT, "no saved Session selected"));
        }
        let settings = local_settings_guard(app)?;
        Some((
            settings.settings.clone(),
            settings.context.clone(),
            settings.tools.clone(),
        ))
    } else {
        None
    };
    let owned = app.0.owned_sessions_only;
    let (next, view, saved) = store_work(move || {
        if let ViewChange::Open(ref session, _) = change {
            if owned && !allow_explicit_member && store.session(&actor, session)?.owner != actor {
                return Err(Error::Denied(
                    "this participant can open only its own private history",
                ));
            }
        }
        let next = match (p, change) {
            (Some(p), ViewChange::New) => p.new_session("New owner conversation")?,
            (Some(p), ViewChange::Open(s, b)) => {
                if allow_explicit_member {
                    p.open_session_with_context_transfer(&s, &b)?
                } else {
                    p.open_session(&s, &b)?
                }
            }
            (Some(p), ViewChange::Fork(n, at)) => p.fork(&n, at.as_deref())?,
            (Some(p), ViewChange::Checkpoint(s)) => p.checkpoint(&s)?,
            (None, ViewChange::Open(s, b)) => {
                let (settings, context, tools) =
                    setup.ok_or(Error::Denied("local Session binding unavailable"))?;
                let binding = sessions::binding(&settings, &context, &tools)?;
                PersistentSession::open(store, actor, binding, &s, &b, false)?
            }
            _ => return Err(Error::Denied("no saved Session selected")),
        };
        let view = next.view()?;
        let saved = saved_selection(&next)?;
        Ok((next, view, saved))
    })
    .await?;
    agent
        .load_history(view.history, Some(Arc::new(next.handle.clone())))
        .map_err(api_error)?;
    *lock(&app.0.session.persistence) = Some(next);
    let snapshot = update_projection(app, &agent, Some(saved), false);
    if !auth_alive(app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
    }
    Ok(Json(snapshot))
}
async fn reset(
    State(app): State<AppState>,
    payload: std::result::Result<Json<ResetCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<Snapshot>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid reset request"))?;
    let durable = lock(&app.0.session.persistence).is_some();
    if durable {
        return change_view(&app, command.expected_revision, ViewChange::New).await;
    }
    let _admission = app.0.session.admission.lock().await;
    if !auth_alive(&app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
    }
    idle_revision(&app, command.expected_revision)?;
    let mut agent = app
        .0
        .session
        .agent
        .try_lock()
        .map_err(|_| ApiError(StatusCode::CONFLICT, "run still active"))?;
    agent.reset().map_err(api_error)?;
    Ok(Json(update_projection(&app, &agent, None, true)))
}
async fn acknowledge(
    State(app): State<AppState>,
    payload: std::result::Result<Json<AcknowledgeCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<Snapshot>> {
    let Json(command) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid acknowledgement"))?;
    let _admission = app.0.session.admission.lock().await;
    if !auth_alive(&app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
    }
    idle_revision(&app, command.expected_revision)?;
    if !command.confirmed_inspected {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "explicit inspection confirmation required",
        ));
    }
    let mut agent = app
        .0
        .session
        .agent
        .try_lock()
        .map_err(|_| ApiError(StatusCode::CONFLICT, "run still active"))?;
    agent.acknowledge_reconciliation(match app.1 {
        Authority::Browser => "Paired browser explicitly confirms inspection of interrupted effects/imported history",
        Authority::Local(_) => "Authenticated same-UID native client explicitly confirms inspection of interrupted effects/imported history",
        Authority::LocalWeb => "Local single-user browser explicitly confirms inspection of interrupted effects/imported history",
    }).await.map_err(api_error)?;
    let persistent = lock(&app.0.session.persistence).clone();
    let saved = if let Some(p) = persistent {
        let (view, saved) = store_work(move || Ok((p.view()?, saved_selection(&p)?))).await?;
        // Retain the existing journal handle; only refresh the global hazard bit.
        let handle = current_persistence(&app)?.handle;
        agent
            .load_history(view.history, Some(Arc::new(handle)))
            .map_err(api_error)?;
        Some(saved)
    } else {
        None
    };
    Ok(Json(update_projection(&app, &agent, saved, false)))
}
async fn session_list(State(app): State<AppState>) -> ApiResult<Response> {
    agent_view::catalog(&app).await
}
async fn branch_list(State(app): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let p = current_persistence(&app)?;
    let branches = store_work(move || p.branches()).await?;
    Ok(Json(serde_json::json!({"branches":branches})))
}
async fn tree_view(State(app): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let p = current_persistence(&app)?;
    let view=store_work(move||Ok(serde_json::json!({"nodes":p.store.tree(&p.actor,p.handle.session_id())?,"branches":p.branches()?}))).await?;
    Ok(Json(view))
}
async fn stored_usage(State(app): State<AppState>) -> ApiResult<Json<aw_store::StoredUsage>> {
    let p = current_persistence(&app)?;
    Ok(Json(
        store_work(move || p.store.usage(&p.actor, p.handle.session_id())).await?,
    ))
}
async fn export_view(State(app): State<AppState>) -> ApiResult<Response> {
    let p = current_persistence(&app)?;
    let bytes = store_work(move || p.store.export_jsonl(&p.actor, p.handle.session_id())).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=aw-session.jsonl",
            ),
        ],
        bytes,
    )
        .into_response())
}
async fn open_session(
    State(app): State<AppState>,
    payload: std::result::Result<Json<OpenCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<Snapshot>> {
    let Json(c) = payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid open request"))?;
    change_view(
        &app,
        c.expected_revision,
        ViewChange::Open(c.session_id, c.branch),
    )
    .await
}
async fn fork_session(
    State(app): State<AppState>,
    payload: std::result::Result<Json<ForkCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<Snapshot>> {
    let Json(c) = payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid fork request"))?;
    change_view(
        &app,
        c.expected_revision,
        ViewChange::Fork(c.name, c.at_node),
    )
    .await
}
async fn checkpoint(
    State(app): State<AppState>,
    payload: std::result::Result<Json<CheckpointCommand>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<Snapshot>> {
    let Json(c) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid checkpoint request"))?;
    change_view(&app, c.expected_revision, ViewChange::Checkpoint(c.summary)).await
}
async fn logout(State(app): State<AppState>) -> ApiResult<Response> {
    if matches!(app.1, Authority::LocalWeb) {
        return Ok(Json(serde_json::json!({
            "locked": false,
            "local_single_user": true
        }))
        .into_response());
    }
    lock(&app.0.auth).cookie_hash = None;
    shutdown_surfaces(&app).await;
    let _ = app.0.session.events.send(WireEvent {
        sequence: 0,
        kind: "auth_expired",
        data: serde_json::json!({}),
    });
    if app.0.local_owner_uid.is_none() {
        if let Some(token) = &lock(&app.0.session.state).cancel {
            token.cancel();
        }
    }
    let mut response=Json(serde_json::json!({"locked":true,"restart_owner_to_pair":true,"local_owner_continues":app.0.local_owner_uid.is_some()})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
            app.0.cookie_name
        ))
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "cannot clear cookie"))?,
    );
    Ok(response)
}
struct EventStream {
    app: AppState,
    receiver: broadcast::Receiver<WireEvent>,
    first: Option<Snapshot>,
    last: u64,
    _permit: OwnedSemaphorePermit,
    terminated: bool,
}
async fn events(
    State(app): State<AppState>,
) -> ApiResult<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>> {
    context_flow::visible(&app).await?;
    let permit = app
        .0
        .event_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "too many event views"))?;
    // Subscribe before taking the snapshot; then discard events at/below its
    // high-water mark. A lagged client gets an explicit fresh snapshot.
    let receiver = app.0.session.events.subscribe();
    let first = lock(&app.0.session.state).snapshot.clone();
    let state = EventStream {
        app,
        receiver,
        last: first.sequence,
        first: Some(first),
        _permit: permit,
        terminated: false,
    };
    let stream = stream::unfold(state, |mut s| async move {
        if s.terminated {
            return None;
        }
        loop {
            if s.app.0.shutdown.is_cancelled() {
                return None;
            }
            if !auth_alive(&s.app) {
                s.terminated = true;
                return Some((Ok(Event::default().event("auth_expired").data("{}")), s));
            }
            if context_flow::visible(&s.app).await.is_err() {
                s.terminated = true;
                return Some((Ok(Event::default().event("context_revoked").data("{}")), s));
            }
            if let Some(first) = s.first.take() {
                let event = Event::default()
                    .event("snapshot")
                    .id(first.sequence.to_string())
                    .data(serde_json::to_string(&first).unwrap_or_else(|_| "{}".into()));
                return Some((Ok(event), s));
            }
            let next = tokio::select! {
                biased;
                _=s.app.0.shutdown.cancelled()=>return None,
                value=s.receiver.recv()=>Some(value),
                _=tokio::time::sleep(Duration::from_secs(15))=>None,
            };
            match next {
                None => return Some((Ok(Event::default().comment("keepalive")), s)),
                Some(Ok(event)) => {
                    if context_flow::visible(&s.app).await.is_err() {
                        s.terminated = true;
                        return Some((Ok(Event::default().event("context_revoked").data("{}")), s));
                    }
                    if event.kind == "auth_expired" {
                        s.terminated = true;
                        return Some((Ok(Event::default().event("auth_expired").data("{}")), s));
                    }
                    if event.sequence <= s.last {
                        continue;
                    }
                    s.last = event.sequence;
                    return Some((
                        Ok(Event::default()
                            .event(event.kind)
                            .id(event.sequence.to_string())
                            .data(event.data.to_string())),
                        s,
                    ));
                }
                Some(Err(broadcast::error::RecvError::Lagged(_))) => {
                    let snapshot = lock(&s.app.0.session.state).snapshot.clone();
                    s.last = snapshot.sequence;
                    s.first = Some(snapshot);
                }
                Some(Err(broadcast::error::RecvError::Closed)) => return None,
            }
        }
    });
    Ok(Sse::new(stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    mod agent_views {
        use super::*;
        include!("web/agent_view_tests.rs");
    }
    use aw_agent_core::{
        AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Message, Provider,
    };
    use axum::body::{to_bytes, Body};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;
    const CODE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HOST: &str = "127.0.0.1:8787";
    #[test]
    fn resource_summary_serializes_only_safe_labels() {
        let value = serde_json::to_value(ResourceSummary {
            context: vec!["system prompt · config · 10 B".into()],
            skills: vec!["review · /local/review.md · 20 B".into()],
        })
        .unwrap();
        assert_eq!(value["context"][0], "system prompt · config · 10 B");
        assert_eq!(value["skills"][0], "review · /local/review.md · 20 B");
        assert!(value.get("text").is_none());
    }

    struct Echo(Arc<AtomicUsize>);
    impl Provider for Echo {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                sink("hi <script> nope")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    ..Default::default()
                })
            })
        }
    }
    fn app() -> (Router, AppState, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let agent = Agent::builder(
            Arc::new(Echo(calls.clone())),
            AgentOptions::new("fixture", "test-model"),
        )
        .build()
        .unwrap();
        let (r, s) = application(agent, HOST.parse().unwrap(), CODE).unwrap();
        (r, s, calls)
    }
    fn local_app() -> (Router, AppState, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("aw-local-web-test-{}", random_token().unwrap()));
        std::fs::create_dir_all(&root).unwrap();
        let context = ExecutionContext::capture(&root).unwrap();
        let uid = context.effective_uid;
        let profile = config::builtins().remove("ollama").unwrap();
        let settings = config::Settings {
            profile_name: "ollama".into(),
            profile,
            system_prompt: "test system".into(),
            request_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(10),
            max_output_bytes: config::MAX_OUTPUT_BYTES,
        };
        let tools = LocalToolOptions {
            root: root.clone(),
            mode: crate::ToolMode::Off,
            selected_tools: Vec::new(),
            execution_site: "local".into(),
            bash: None,
        };
        let agent = crate::unconfigured_execution(&settings, &context, &tools).unwrap();
        let local = LocalWebOptions {
            config_path: Some(root.join("config.toml")),
            file_config: config::FileConfig::default(),
            profiles: config::builtins(),
            settings,
            context,
            tools,
            state_dir: None,
            store: None,
            title: None,
            domain_selector: None,
            workspace_selector: None,
            resources: ResourceSummary::default(),
        };
        let (router, state) = application_scoped(
            agent,
            None,
            ApplicationScope {
                workspace_options: None,
                address: HOST.parse().unwrap(),
                pairing_code: CODE.to_owned(),
                local_owner_uid: uid,
                owned_sessions_only: false,
                authority: Authority::LocalWeb,
                local_settings: Some(local),
            },
        )
        .unwrap();
        (router, state, root)
    }

    fn request(method: &str, path: &str, body: serde_json::Value, cookie: Option<&str>) -> Request {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header("host", HOST)
            .header("origin", format!("http://{HOST}"))
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            b = b.header("cookie", cookie);
        }
        b.body(Body::from(body.to_string())).unwrap()
    }
    async fn paired(r: &Router) -> String {
        let response = r
            .clone()
            .oneshot(request(
                "POST",
                "/api/pair",
                serde_json::json!({"code":CODE}),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        cookie.split(';').next().unwrap().into()
    }
    #[tokio::test]
    async fn public_ui_has_strict_headers_and_no_provider_call() {
        let (r, _, c) = app();
        let response = r
            .oneshot(request("GET", "/", serde_json::Value::Null, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'"));
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn session_requires_auth() {
        let (r, _, _) = app();
        assert_eq!(
            r.oneshot(request(
                "GET",
                "/api/session",
                serde_json::Value::Null,
                None
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn local_single_user_needs_no_pairing_and_can_open_settings_unconfigured() {
        let (router, _, root) = local_app();
        let session = router
            .clone()
            .oneshot(request(
                "GET",
                "/api/session",
                serde_json::Value::Null,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::OK);
        let body = to_bytes(session.into_body(), 1024 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(value["identity"].is_null());
        assert_eq!(value["accounts"], serde_json::json!([]));

        let settings = router
            .clone()
            .oneshot(request(
                "GET",
                "/api/settings",
                serde_json::Value::Null,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(settings.status(), StatusCode::OK);
        let body = to_bytes(settings.into_body(), 1024 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["local_single_user"], true);

        let pair = router
            .oneshot(request(
                "POST",
                "/api/pair",
                serde_json::json!({"code":CODE}),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(pair.status(), StatusCode::CONFLICT);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn local_settings_default_requires_a_model_and_does_not_publish_partial_config() {
        let (router, _, root) = local_app();
        let response = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/settings/profile",
                serde_json::json!({
                    "name":"empty",
                    "kind":"ollama",
                    "base_url":"http://127.0.0.1:11434",
                    "default_model":null,
                    "models":["candidate-only"],
                    "api_key_env":null,
                    "allow_insecure_http":false,
                    "make_default":true
                }),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let settings = router
            .oneshot(request(
                "GET",
                "/api/settings",
                serde_json::Value::Null,
                None,
            ))
            .await
            .unwrap();
        let body = to_bytes(settings.into_body(), 1024 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(value["default_profile"].is_null());
        assert!(!value["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| profile["name"] == "empty"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn local_settings_register_default_without_dispatching_provider() {
        let (router, _, root) = local_app();
        let response = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/settings/profile",
                serde_json::json!({
                    "name":"local-test",
                    "kind":"ollama",
                    "base_url":"http://127.0.0.1:11434",
                    "default_model":"fixture-model",
                    "models":["fixture-small"],
                    "api_key_env":"LOCAL_TEST_KEY",
                    "allow_insecure_http":false,
                    "make_default":true
                }),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let session = router
            .oneshot(request(
                "GET",
                "/api/session",
                serde_json::Value::Null,
                None,
            ))
            .await
            .unwrap();
        let body = to_bytes(session.into_body(), 1024 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["profile"], "local-test");
        assert_eq!(value["model"], "fixture-model");
        assert_eq!(value["identity"]["account_id"], "local-test");
        assert!(value["saved"].is_null());
        let config_text = std::fs::read_to_string(root.join("config.toml")).unwrap();
        assert!(config_text.contains("LOCAL_TEST_KEY"));
        assert!(!config_text.contains("__unconfigured"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn local_settings_ui_round_trips_shared_tui_preferences() {
        let (router, _, root) = local_app();
        let response = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/settings/ui",
                serde_json::json!({
                    "status_lines":[
                        {"region":"above-composer","items":["state"]},
                        {"region":"below-composer","items":["directory","branch"]},
                        {"region":"below-composer","items":["model","profile"]},
                        {"region":"fullscreen-status","items":["model","branch"]}
                    ],
                    "tui_mode":"fullscreen",
                    "theme":"dark",
                    "editor_padding_x":0,
                    "output_padding":0,
                    "autocomplete_max_visible":7,
                    "show_performance":true,
                    "show_ttft":true,
                    "show_waterfall":false,
                    "show_thinking":false,
                    "show_tool_calls":false
                }),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["ui"]["tui_mode"], "fullscreen");
        assert_eq!(value["ui"]["theme"], "dark");
        assert_eq!(value["ui"]["editor_padding_x"], 0);
        assert_eq!(value["ui"]["output_padding"], 0);
        assert_eq!(value["ui"]["autocomplete_max_visible"], 7);
        assert_eq!(value["ui"]["show_performance"], true);
        assert_eq!(value["ui"]["show_ttft"], true);
        assert_eq!(value["ui"]["show_waterfall"], false);
        assert_eq!(value["ui"]["show_thinking"], false);
        assert_eq!(value["ui"]["show_tool_calls"], false);
        assert_eq!(
            value["ui"]["status_lines"],
            serde_json::json!([
                {"region":"above-composer","items":["state"]},
                {"region":"below-composer","items":["directory","branch"]},
                {"region":"below-composer","items":["model","profile"]},
                {"region":"fullscreen-status","items":["model","branch"]}
            ])
        );

        let loaded = config::load(Some(&root.join("config.toml"))).unwrap();
        assert_eq!(loaded.ui.tui_mode, config::TuiMode::Fullscreen);
        assert_eq!(loaded.ui.theme, config::ThemeMode::Dark);
        assert_eq!(loaded.ui.autocomplete_max_visible, 7);
        assert!(loaded.ui.show_ttft);
        assert!(!loaded.ui.show_waterfall);
        assert!(!loaded.ui.show_thinking);
        assert!(!loaded.ui.show_tool_calls);

        let invalid = router
            .oneshot(request(
                "POST",
                "/api/settings/ui",
                serde_json::json!({
                    "status_lines":[
                        {"region":"below-composer","items":["model","model"]}
                    ],
                    "tui_mode":"inline",
                    "theme":"system",
                    "editor_padding_x":0,
                    "output_padding":0,
                    "autocomplete_max_visible":5,
                    "show_performance":true,
                    "show_ttft":false,
                    "show_waterfall":false,
                    "show_thinking":true,
                    "show_tool_calls":true
                }),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        let loaded = config::load(Some(&root.join("config.toml"))).unwrap();
        assert_eq!(loaded.ui.status_lines.len(), 4);
        assert_eq!(
            loaded.ui.status_lines[1].items,
            vec![config::StatusItem::Directory, config::StatusItem::Branch]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn paired_browser_cannot_mutate_local_settings() {
        let (router, _, _) = app();
        let cookie = paired(&router).await;
        let response = router
            .oneshot(request(
                "GET",
                "/api/settings",
                serde_json::Value::Null,
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn refuses_host_rebinding() {
        let (r, _, _) = app();
        let mut req = request("GET", "/", serde_json::Value::Null, None);
        req.headers_mut().insert(
            header::HOST,
            HeaderValue::from_static("attacker.invalid:8787"),
        );
        assert_eq!(
            r.oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn rejects_other_port_origin() {
        let (r, _, _) = app();
        let mut req = request("POST", "/api/pair", serde_json::json!({"code":CODE}), None);
        req.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:9999"),
        );
        assert_eq!(
            r.oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn rejects_originless_mutation() {
        let (r, _, _) = app();
        let mut req = request("POST", "/api/pair", serde_json::json!({"code":CODE}), None);
        req.headers_mut().remove(header::ORIGIN);
        assert_eq!(
            r.oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn pairing_is_single_use() {
        let (r, _, _) = app();
        paired(&r).await;
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/pair",
                serde_json::json!({"code":CODE}),
                None
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn expired_pairing_is_refused() {
        let (r, s, _) = app();
        lock(&s.0.auth).pairing_expires = Instant::now();
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/pair",
                serde_json::json!({"code":CODE}),
                None
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn cookie_and_snapshot_do_not_expose_provider_secrets() {
        let (r, _, _) = app();
        let cookie = paired(&r).await;
        let res = r
            .oneshot(request(
                "GET",
                "/api/session",
                serde_json::Value::Null,
                Some(&cookie),
            ))
            .await
            .unwrap();
        let b = to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["model"], "test-model");
        assert!(v.get("base_url").is_none() && v.get("api_key_env").is_none());
    }
    #[tokio::test]
    async fn duplicate_prompt_dispatches_once() {
        let (r, s, c) = app();
        let cookie = paired(&r).await;
        let command =
            serde_json::json!({"request_id":"id1","expected_revision":0,"prompt":"hello"});
        for _ in 0..2 {
            assert_eq!(
                r.clone()
                    .oneshot(request(
                        "POST",
                        "/api/prompts",
                        command.clone(),
                        Some(&cookie)
                    ))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::ACCEPTED
            );
        }
        for _ in 0..100 {
            if lock(&s.0.session.state).snapshot.pending.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(c.load(Ordering::SeqCst), 1);
        assert_eq!(lock(&s.0.session.state).snapshot.turns.len(), 1);
    }
    #[tokio::test]
    async fn reused_id_for_different_input_conflicts() {
        let (r, _, _) = app();
        let cookie = paired(&r).await;
        for text in ["one", "two"] {
            let response = r
                .clone()
                .oneshot(request(
                    "POST",
                    "/api/prompts",
                    serde_json::json!({"request_id":"same","expected_revision":0,"prompt":text}),
                    Some(&cookie),
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if text == "one" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::CONFLICT
                }
            );
        }
    }
    #[tokio::test]
    async fn stale_revision_does_not_dispatch() {
        let (r, _, c) = app();
        let cookie = paired(&r).await;
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/prompts",
                serde_json::json!({"request_id":"bad","expected_revision":5,"prompt":"hello"}),
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn reset_is_revision_checked() {
        let (r, _, _) = app();
        let cookie = paired(&r).await;
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/reset",
                serde_json::json!({"expected_revision":20}),
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::CONFLICT
        );
    }
    #[tokio::test]
    async fn logout_revokes_cookie() {
        let (r, _, _) = app();
        let cookie = paired(&r).await;
        r.clone()
            .oneshot(request(
                "POST",
                "/api/logout",
                serde_json::json!({}),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(
            r.oneshot(request(
                "GET",
                "/api/session",
                serde_json::Value::Null,
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn no_arbitrary_provider_fields_allowed() {
        let (r, _, c) = app();
        let cookie = paired(&r).await;
        assert_eq!(r.oneshot(request("POST","/api/prompts",serde_json::json!({"request_id":"x","expected_revision":0,"prompt":"hi","base_url":"https://attacker.invalid"}),Some(&cookie))).await.unwrap().status(),StatusCode::BAD_REQUEST);
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn refuses_nonloopback_configuration() {
        let calls = Arc::new(AtomicUsize::new(0));
        let a = Agent::builder(Arc::new(Echo(calls)), AgentOptions::new("p", "m"))
            .build()
            .unwrap();
        assert!(application(a, "0.0.0.0:8787".parse().unwrap(), CODE).is_err());
    }
    #[tokio::test]
    async fn forged_principal_and_credentials_are_not_prompt_fields() {
        for field in ["actor", "user_id", "api_key", "credential", "base_url"] {
            let (r, _, calls) = app();
            let cookie = paired(&r).await;
            let mut body =
                serde_json::json!({"request_id":"x","expected_revision":0,"prompt":"hi"});
            body[field] = serde_json::json!("forged");
            let response = r
                .oneshot(request("POST", "/api/prompts", body, Some(&cookie)))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }
    #[tokio::test]
    async fn unauthorized_account_does_not_fallback_to_default() {
        let (r, _, calls) = app();
        let cookie = paired(&r).await;
        let response=r.oneshot(request("POST","/api/prompts",serde_json::json!({"request_id":"x","expected_revision":0,"prompt":"hi","account_id":"someone-else","model":"test-model"}),Some(&cookie))).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn alternate_granted_account_records_actual_actor_and_payer() {
        let (r, state, default_calls) = app();
        let cookie = paired(&r).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let account = aw_agent_core::AccountDescriptor {
            id: "team".into(),
            owner: aw_agent_core::AccountOwner::Server("owner".into()),
            payer: aw_agent_core::BillingOwner::Shared("team-budget".into()),
            credential_custody: aw_agent_core::CredentialCustody::Server("local".into()),
            credential_version: None,
            inference_executor: "local".into(),
            allowed_models: std::collections::BTreeSet::from(["team-model".into()]),
            grants: std::collections::BTreeSet::from([state.0.session.actor.clone()]),
            shared_with_all_authenticated: false,
        };
        state
            .0
            .session
            .accounts
            .register(account, Arc::new(Echo(calls.clone())))
            .unwrap();
        let response=r.clone().oneshot(request("POST","/api/prompts",serde_json::json!({"request_id":"x","expected_revision":0,"prompt":"hi","account_id":"team","model":"team-model"}),Some(&cookie))).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        for _ in 0..100 {
            if lock(&state.0.session.state).snapshot.pending.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(default_calls.load(Ordering::SeqCst), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let s = lock(&state.0.session.state);
        let identity = &s.snapshot.turns[0].invocations[0].identity;
        assert_eq!(identity.actor, state.0.session.actor);
        assert_eq!(identity.account_id, "team");
        assert_eq!(
            identity.payer,
            aw_agent_core::BillingOwner::Shared("team-budget".into())
        );
    }
    #[tokio::test]
    async fn account_listing_omits_another_personal_account() {
        let (r, state, _) = app();
        let cookie = paired(&r).await;
        let other = aw_agent_core::PrincipalId::new("other").unwrap();
        state
            .0
            .session
            .accounts
            .register(
                aw_agent_core::AccountDescriptor::personal("hidden", other, "local", ["m".into()]),
                Arc::new(Echo(Arc::new(AtomicUsize::new(0)))),
            )
            .unwrap();
        let response = r
            .oneshot(request(
                "GET",
                "/api/accounts",
                serde_json::Value::Null,
                Some(&cookie),
            ))
            .await
            .unwrap();
        let data = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&data).contains("hidden"));
    }
    #[tokio::test]
    async fn reset_does_not_acknowledge_unknown_effects() {
        let (r, state, _) = app();
        let cookie = paired(&r).await;
        lock(&state.0.session.state).snapshot.needs_reconciliation = true;
        let response = r
            .clone()
            .oneshot(request(
                "POST",
                "/api/reset",
                serde_json::json!({"expected_revision":0}),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(lock(&state.0.session.state).snapshot.needs_reconciliation);
        let revision = lock(&state.0.session.state).snapshot.revision;
        let response = r
            .oneshot(request(
                "POST",
                "/api/acknowledge",
                serde_json::json!({"expected_revision":revision,"confirmed_inspected":false}),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    fn persistent_app() -> (Router, AppState, Arc<AtomicUsize>, PersistentSession) {
        let calls = Arc::new(AtomicUsize::new(0));
        let agent = Agent::builder(
            Arc::new(Echo(calls.clone())),
            AgentOptions::new("fixture", "test-model"),
        )
        .build()
        .unwrap();
        let store = aw_store::SessionStore::in_memory().unwrap();
        let binding = aw_store::SessionBinding {
            working_directory: "/fixture".into(),
            tool_root: "/fixture".into(),
            system_prompt: String::new(),
            profile_hint: "fixture".into(),
            model_hint: "test-model".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let persistent = PersistentSession::create(
            store,
            agent.actor().clone(),
            binding,
            "saved browser fixture",
        )
        .unwrap();
        let (r, s) =
            application_with_state(agent, Some(persistent.clone()), HOST.parse().unwrap(), CODE)
                .unwrap();
        (r, s, calls, persistent)
    }
    async fn wait_settled(s: &AppState) {
        for _ in 0..300 {
            if lock(&s.0.session.state).snapshot.pending.is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        panic!("fixture run did not settle");
    }
    fn saved_prompt(s: &AppState, id: &str) -> serde_json::Value {
        let state = lock(&s.0.session.state);
        let saved = state.snapshot.saved.as_ref().unwrap();
        serde_json::json!({"request_id":id,"expected_revision":state.snapshot.revision,"session_id":saved.session_id,"branch_id":saved.branch_id,"prompt":"saved hello"})
    }
    #[tokio::test]
    async fn persistent_send_requires_exact_session_and_branch() {
        let (r, _, calls, _) = persistent_app();
        let cookie = paired(&r).await;
        let response = r
            .oneshot(request(
                "POST",
                "/api/prompts",
                serde_json::json!({"request_id":"missing","expected_revision":0,"prompt":"hello"}),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn persistent_send_is_in_tree_and_receipt_before_retry() {
        let (r, s, calls, p) = persistent_app();
        let cookie = paired(&r).await;
        let body = saved_prompt(&s, "req1");
        assert_eq!(
            r.clone()
                .oneshot(request("POST", "/api/prompts", body.clone(), Some(&cookie)))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
        wait_settled(&s).await;
        assert_eq!(
            p.store.tree(&p.actor, p.handle.session_id()).unwrap().len(),
            3
        );
        assert_eq!(
            r.oneshot(request("POST", "/api/prompts", body, Some(&cookie)))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn persistent_new_keeps_history_and_receipt() {
        let (r, s, calls, p) = persistent_app();
        let cookie = paired(&r).await;
        let body = saved_prompt(&s, "req1");
        r.clone()
            .oneshot(request("POST", "/api/prompts", body.clone(), Some(&cookie)))
            .await
            .unwrap();
        wait_settled(&s).await;
        let rev = lock(&s.0.session.state).snapshot.revision;
        assert_eq!(
            r.clone()
                .oneshot(request(
                    "POST",
                    "/api/reset",
                    serde_json::json!({"expected_revision":rev}),
                    Some(&cookie)
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(p.store.list(&p.actor).unwrap().len(), 2);
        assert_eq!(
            r.oneshot(request("POST", "/api/prompts", body, Some(&cookie)))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn persistent_fork_is_view_change_not_execution() {
        let (r, s, calls, p) = persistent_app();
        let cookie = paired(&r).await;
        let res = r
            .oneshot(request(
                "POST",
                "/api/fork",
                serde_json::json!({"expected_revision":0,"name":"alternative"}),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            lock(&s.0.session.state)
                .snapshot
                .saved
                .as_ref()
                .unwrap()
                .branch_name,
            "alternative"
        );
        assert_eq!(
            p.store
                .branches(&p.actor, p.handle.session_id())
                .unwrap()
                .len(),
            2
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn persistent_checkpoint_is_explicit_and_revision_guarded() {
        let (r, s, calls, _) = persistent_app();
        let cookie = paired(&r).await;
        assert_eq!(
            r.clone()
                .oneshot(request(
                    "POST",
                    "/api/checkpoint",
                    serde_json::json!({"expected_revision":99,"summary":"bad stale"}),
                    Some(&cookie)
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/checkpoint",
                serde_json::json!({"expected_revision":0,"summary":"Reviewed requirements only"}),
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::OK
        );
        assert!(lock(&s.0.session.state).snapshot.context_checkpoint);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn persisted_session_apis_still_require_pairing() {
        let (r, _, _, _) = persistent_app();
        for path in [
            "/api/sessions",
            "/api/branches",
            "/api/tree",
            "/api/export",
            "/api/usage",
        ] {
            assert_eq!(
                r.clone()
                    .oneshot(request("GET", path, serde_json::Value::Null, None))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
    }
    #[tokio::test]
    async fn persistent_list_does_not_export_private_binding_fields() {
        let (r, _, _, _) = persistent_app();
        let cookie = paired(&r).await;
        let response = r
            .oneshot(request(
                "GET",
                "/api/sessions",
                serde_json::Value::Null,
                Some(&cookie),
            ))
            .await
            .unwrap();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("destination_fingerprint")
                && !text.contains("system_prompt")
                && !text.contains("api_key_env")
        );
    }

    #[cfg(feature = "surfaces")]
    struct SurfaceFixture(std::path::PathBuf);
    #[cfg(feature = "surfaces")]
    impl SurfaceFixture {
        fn new() -> Self {
            use std::os::unix::fs::DirBuilderExt;
            let p =
                std::env::temp_dir().join(format!("aw-web-surface-{}", random_token().unwrap()));
            std::fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
            std::fs::write(p.join("sample.rs"), "<script>not HTML</script>\n").unwrap();
            Self(p)
        }
    }
    #[cfg(feature = "surfaces")]
    impl Drop for SurfaceFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[cfg(feature = "surfaces")]
    fn surface_app() -> (Router, AppState, Arc<AtomicUsize>, SurfaceFixture) {
        let root = SurfaceFixture::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let agent = Agent::builder(
            Arc::new(Echo(calls.clone())),
            AgentOptions::new("fixture", "test-model"),
        )
        .build()
        .unwrap();
        let options = WorkspaceOptions {
            root: root.0.clone(),
            git: std::path::PathBuf::from("/usr/bin/git"),
            bash: std::path::PathBuf::from("/bin/bash"),
            actor: "not-a-browser-supplied-actor".into(),
            allow_terminals: false,
            sites: vec![
                aw_surfaces::Site::new("docs", "Docs", "https://docs.example.test/").unwrap(),
            ],
        };
        let (r, s) =
            application_with_workspace(agent, None, Some(options), HOST.parse().unwrap(), CODE)
                .unwrap();
        (r, s, calls, root)
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surfaces_off_without_host_grant() {
        let (r, _, c) = app();
        let cookie = paired(&r).await;
        let response = r
            .clone()
            .oneshot(request(
                "GET",
                "/api/workspace",
                serde_json::Value::Null,
                Some(&cookie),
            ))
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["enabled"],
            false
        );
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/workspace/action",
                serde_json::json!({"op":"files"}),
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surfaces_require_authentication() {
        let (r, _, _, _root) = surface_app();
        for method in ["GET", "POST"] {
            let path = if method == "GET" {
                "/api/workspace"
            } else {
                "/api/workspace/action"
            };
            assert_eq!(
                r.clone()
                    .oneshot(request(
                        method,
                        path,
                        serde_json::json!({"op":"capabilities"}),
                        None
                    ))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_actor_is_taken_from_owner() {
        let (r, s, c, _root) = surface_app();
        let cookie = paired(&r).await;
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/workspace/action",
                serde_json::json!({"op":"capabilities"}),
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::OK
        );
        assert!(s
            .0
            .surfaces
            .as_ref()
            .unwrap()
            .execute(
                "not-a-browser-supplied-actor",
                aw_surfaces::Command::Capabilities
            )
            .is_err());
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_file_read_is_not_a_model_call() {
        let (r, _, c, _root) = surface_app();
        let cookie = paired(&r).await;
        let response = r
            .oneshot(request(
                "POST",
                "/api/workspace/action",
                serde_json::json!({"op":"file","path":"sample.rs"}),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["text"], "<script>not HTML</script>\n");
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_files_are_from_fixed_root() {
        let (r, _, c, _root) = surface_app();
        let cookie = paired(&r).await;
        let response = r
            .oneshot(request(
                "POST",
                "/api/workspace/action",
                serde_json::json!({"op":"files","path":""}),
                Some(&cookie),
            ))
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["entries"][0]["name"], "sample.rs");
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_rejects_forged_authority_fields() {
        for field in ["actor", "root", "endpoint", "api_key", "grant"] {
            let (r, _, _, _root) = surface_app();
            let cookie = paired(&r).await;
            let mut body = serde_json::json!({"op":"files","path":""});
            body[field] = serde_json::json!("forged");
            assert_eq!(
                r.oneshot(request(
                    "POST",
                    "/api/workspace/action",
                    body,
                    Some(&cookie)
                ))
                .await
                .unwrap()
                .status(),
                StatusCode::BAD_REQUEST
            );
        }
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_cannot_be_generic_fetch_proxy() {
        let (r, _, _, _root) = surface_app();
        let cookie = paired(&r).await;
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/workspace/action",
                serde_json::json!({"op":"fetch","url":"http://127.0.0.1/private"}),
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_shell_needs_owner_grant() {
        let (r, _, _, _root) = surface_app();
        let cookie = paired(&r).await;
        assert_eq!(r.oneshot(request("POST","/api/workspace/action",serde_json::json!({"op":"terminal_start","request_id":"one","context":"root","confirmed":true,"rows":24,"cols":80}),Some(&cookie))).await.unwrap().status(),StatusCode::FORBIDDEN);
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_origin_boundary_is_unchanged() {
        let (r, _, _, _root) = surface_app();
        let cookie = paired(&r).await;
        let mut req = request(
            "POST",
            "/api/workspace/action",
            serde_json::json!({"op":"files"}),
            Some(&cookie),
        );
        req.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("https://other.invalid"),
        );
        assert_eq!(
            r.oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn site_csp_only_adds_registered_origins() {
        let (r, _, _, _root) = surface_app();
        let response = r
            .oneshot(request("GET", "/", serde_json::Value::Null, None))
            .await
            .unwrap();
        let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(csp.contains("frame-src https://docs.example.test"));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.contains("connect-src 'self'"));
        assert!(!csp.contains("script-src *"));
    }
    #[tokio::test]
    async fn disabled_workspace_csp_keeps_default() {
        let (r, _, _) = app();
        let response = r
            .oneshot(request("GET", "/app.js", serde_json::Value::Null, None))
            .await
            .unwrap();
        assert_eq!(response.headers()[header::CONTENT_SECURITY_POLICY], CSP);
    }
    #[tokio::test]
    async fn html_style_nonce_is_fresh_response_local_and_does_not_enable_scripts_or_attributes() {
        let (router, _, calls) = app();
        let mut previous: Option<String> = None;
        for _ in 0..2 {
            let response = router
                .clone()
                .oneshot(request("GET", "/", serde_json::Value::Null, None))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .to_owned();
            let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            let html = String::from_utf8(body.to_vec()).unwrap();
            let nonce = html
                .split_once("name=\"aw-style-nonce\" content=\"")
                .unwrap()
                .1
                .split_once('"')
                .unwrap()
                .0;
            assert!(valid_token(nonce));
            assert!(!html.contains(STYLE_NONCE_PLACEHOLDER));
            assert_eq!(csp.replace(&format!(" 'nonce-{nonce}'"), ""), CSP);
            assert!(csp.contains("script-src 'self';"));
            assert!(!csp.contains("'unsafe-inline'"));
            if let Some(previous) = &previous {
                assert_ne!(nonce, previous.as_str());
            }
            previous = Some(nonce.to_owned());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surfaces_do_not_survive_logout_authority() {
        let (r, s, _, _root) = surface_app();
        let cookie = paired(&r).await;
        let host = s.0.surfaces.clone().unwrap();
        assert_eq!(
            r.oneshot(request(
                "POST",
                "/api/logout",
                serde_json::json!({}),
                Some(&cookie)
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::OK
        );
        assert!(host
            .execute(s.0.session.actor.as_str(), aw_surfaces::Command::Terminals)
            .is_err());
    }
    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn observed_surface_never_changes_agent_configuration() {
        let (r, s, c, _root) = surface_app();
        let cookie = paired(&r).await;
        let before = lock(&s.0.session.state).snapshot.identity.clone();
        r.oneshot(request(
            "POST",
            "/api/workspace/action",
            serde_json::json!({"op":"files","path":""}),
            Some(&cookie),
        ))
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(lock(&s.0.session.state).snapshot.identity.clone()).unwrap()
        );
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn theme_bootstrap_assets_are_same_origin_and_strict_csp_safe() {
        let (router, _, calls) = app();
        for (path, content_type, marker) in [
            (
                "/theme-bootstrap.js",
                "text/javascript; charset=utf-8",
                "__awThemeReady",
            ),
            (
                "/theme-bootstrap.css",
                "text/css; charset=utf-8",
                "#aw-boot",
            ),
        ] {
            let response = router
                .clone()
                .oneshot(request("GET", path, serde_json::Value::Null, None))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], content_type);
            assert_eq!(response.headers()[header::CONTENT_SECURITY_POLICY], CSP);
            let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            assert!(String::from_utf8_lossy(&body).contains(marker));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn embedded_browser_shell_is_solid_abird_and_workspace_first() {
        let app = include_str!("../web/src/main.jsx");
        let styles = include_str!("../web/src/styles.css");
        let package = include_str!("../web/package.json");
        let built = include_str!("../web/dist/index.html");
        assert!(package.contains("solid-js"));
        assert!(app.contains("SessionNavigator"));
        assert!(app.contains("GitNavigator"));
        assert!(app.contains("FilesNavigator"));
        assert!(app.contains("ChatGPT subscription"));
        assert!(app.contains("openai-responses"));
        assert!(app.contains("Ollama Cloud"));
        assert!(app.contains("Presentation"));
        assert!(app.contains("Status rows"));
        assert!(app.contains("status_lines"));
        assert!(app.contains("Save presentation"));
        assert!(app.contains("TPS / performance"));
        assert!(styles.contains("--sidebar-width: 286px"));
        assert!(styles.contains("#60b786"));
        assert!(styles.contains(".command-bar"));
        assert!(styles.contains(".document-tabs"));
        assert!(styles.contains(".sidebar-resizer"));
        assert!(styles.contains(".navigator-search"));
        assert!(styles.contains(".composer-dock"));
        assert!(styles.contains(".provider-grid"));
        assert!(app.contains("CodeFile"));
        assert!(app.contains("CodeDiff"));
        assert!(app.contains("file_search"));
        assert!(built.contains("/theme-bootstrap.css"));
        assert!(built.contains("/theme-bootstrap.js"));
        assert!(built.contains("/app.js"));
        assert!(built.contains("/style.css"));
        assert!(built.find("/theme-bootstrap.css") < built.find("/theme-bootstrap.js"));
        assert!(built.find("/theme-bootstrap.js") < built.find("/app.js"));
        assert!(!built.contains("<style"));
        assert!(built.match_indices("<script").all(|(at, _)| {
            built[at..]
                .split_once('>')
                .is_some_and(|(tag, _)| tag.contains(" src="))
        }));
        assert!(!built.contains("node_modules"));
        let editor_source = include_str!("../web/src/code-surface.jsx");
        assert!(editor_source.contains("MergeView"));
        assert!(editor_source.contains("github-dark-default"));
        let editor = include_str!("../web/dist/chunk-code-surface.js");
        assert!(editor.len() > 100_000);
        assert!(editor.contains("github-dark-default"));
    }

    #[cfg(feature = "surfaces")]
    #[tokio::test]
    async fn surface_static_assets_are_embedded_without_secrets() {
        let (r, _, _, _root) = surface_app();
        for path in [
            "/workspace.js",
            "/workspace-sidebars.js",
            "/workspace-model.js",
            "/workspace.css",
            "/settings.js",
        ] {
            let response = r
                .clone()
                .oneshot(request("GET", path, serde_json::Value::Null, None))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), 200_000).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains(CODE));
        }
    }
}
