//! AW-P6-9C2: opt-in signed MachineLink QUIC for an existing original Actor.
//! Peer Node identity AND current explicit Session grant AND current writer /
//! Actor Envelope all must match per operation. No alternate Agent executor.
use super::{
    machinelink::{
        negotiate_client, negotiate_server, validate_caps, validate_lease, MachineCapability,
        MachineLinkConnectOptions, MachineLinkLease, MachineLinkServeOptions,
    },
    reachability::open_punched_quic,
    *,
};
use aw::{
    actor_owner::{public_operation, ActorOperation, ActorOperationResult, Service},
    Agent, PrincipalId,
};
use aw_agent_core::CancellationToken;
use aw_store::{ActorControlAction, ActorControlState, ActorPeerPermission, QueueTarget};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::{self, BufRead},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, Semaphore},
    task::JoinSet,
};

const VERSION: u16 = 1;
const MAX_REQUEST: usize = 192 * 1024;
const MAX_REPLY: usize = 640 * 1024;
const MAX_FRAMES: usize = 128;
const MAX_CONCURRENT: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActorRemoteRequest {
    Query {
        operation: Value,
    },
    Inspect {
        operation: Value,
    },
    Enqueue {
        request_id: String,
        message_id: u64,
        text: String,
        #[serde(default)]
        priority: i8,
        target_turn: Option<u64>,
    },
    Step {},
    Control {
        request_id: String,
        message_id: u64,
        action: ActorControlAction,
    },
    NoticeRead {
        request_id: String,
    },
}
impl ActorRemoteRequest {
    pub fn capability(&self) -> MachineCapability {
        match self {
            Self::Query { .. } => MachineCapability::ActorQuery,
            Self::Inspect { .. } => MachineCapability::ActorInspect,
            Self::Enqueue { .. } => MachineCapability::ActorEnqueue,
            Self::Step {} => MachineCapability::ActorStep,
            Self::Control { .. } => MachineCapability::ActorControl,
            Self::NoticeRead { .. } => MachineCapability::ActorNoticeRead,
        }
    }
    pub fn permission(&self) -> ActorPeerPermission {
        match self {
            Self::Query { .. } => ActorPeerPermission::Query,
            Self::Inspect { .. } => ActorPeerPermission::Inspect,
            Self::Enqueue { .. } => ActorPeerPermission::Enqueue,
            Self::Step {} => ActorPeerPermission::Step,
            Self::Control { .. } => ActorPeerPermission::Control,
            Self::NoticeRead { .. } => ActorPeerPermission::NoticeRead,
        }
    }
    fn validate(&self) -> Result<()> {
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-:.@".contains(&b))
        };
        match self {
            Self::Query { operation } | Self::Inspect { operation } => {
                if serde_json::to_vec(operation).map_or(true, |v| v.len() > 16 * 1024) {
                    return Err(Error::Limit("Actor remote operation metadata too large"));
                }
                ActorOperation::from_rpc_value(operation.clone())?;
            }
            Self::Enqueue {
                request_id,
                message_id,
                text,
                priority,
                target_turn,
            } if !token(request_id)
                || *message_id == 0
                || text.is_empty()
                || text.len() > 64 * 1024
                || text.contains('\0')
                || !(-20..=19).contains(priority)
                || target_turn == &Some(0) =>
            {
                return Err(Error::Denied("invalid Actor remote encrypted Input"))
            }
            Self::Control {
                request_id,
                message_id,
                ..
            } if !token(request_id) || *message_id == 0 => {
                return Err(Error::Denied("invalid remote Actor current-Turn Control"))
            }
            Self::NoticeRead { request_id } if !token(request_id) => {
                return Err(Error::Denied("invalid Actor private Notice ID"))
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    version: u16,
    lease_id: String,
    request_id: String,
    session_id: String,
    branch_id: String,
    request: ActorRemoteRequest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireResponse {
    version: u16,
    lease_id: String,
    request_id: String,
    status: String,
    result: Option<Value>,
    code: Option<String>,
    needs_inspection: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ActorRemoteResult {
    pub lease: MachineLinkLease,
    pub status: String,
    pub result: Option<Value>,
    pub code: Option<String>,
    pub needs_inspection: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ActorRemoteServed {
    pub lease: MachineLinkLease,
    pub handled: usize,
}
fn is_actor_capability(cap: MachineCapability) -> bool {
    matches!(
        cap,
        MachineCapability::ActorQuery
            | MachineCapability::ActorInspect
            | MachineCapability::ActorEnqueue
            | MachineCapability::ActorStep
            | MachineCapability::ActorControl
            | MachineCapability::ActorNoticeRead
    )
}
fn new_request_id() -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b)
        .map_err(|_| Error::Storage("OS randomness unavailable for Actor MachineLink"))?;
    Ok(format!("arq_{}", hex(&b)))
}

async fn read_actor_frame<T: for<'de> Deserialize<'de>>(
    recv: &mut quinn::RecvStream,
    limit: usize,
) -> Result<T> {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|_| Error::Transport("cannot read Actor QUIC frame length"))?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > limit {
        return Err(Error::Limit("Actor MachineLink frame exceeds bounded size"));
    }
    let mut data = vec![0u8; length];
    recv.read_exact(&mut data)
        .await
        .map_err(|_| Error::Transport("cannot read Actor QUIC frame bytes"))?;
    serde_json::from_slice(&data)
        .map_err(|_| Error::Protocol("invalid strictly typed Actor QUIC frame"))
}
async fn write_actor_frame<T: Serialize>(
    send: &mut quinn::SendStream,
    body: &T,
    limit: usize,
) -> Result<()> {
    let bytes = serde_json::to_vec(body)
        .map_err(|_| Error::Protocol("cannot serialize Actor QUIC frame"))?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err(Error::Limit("Actor QUIC result exceeds frame limit"));
    }
    let n = u32::try_from(bytes.len()).map_err(|_| Error::Limit("Actor frame length overflow"))?;
    send.write_all(&n.to_be_bytes())
        .await
        .map_err(|_| Error::Transport("Actor QUIC frame write failed"))?;
    send.write_all(&bytes)
        .await
        .map_err(|_| Error::Transport("Actor QUIC frame write failed"))?;
    send.finish()
        .map_err(|_| Error::Transport("Actor QUIC frame finish failed"))?;
    if send
        .stopped()
        .await
        .map_err(|_| Error::Transport("Actor QUIC frame outcome uncertain"))?
        .is_some()
    {
        return Err(Error::Transport(
            "Actor QUIC peer stopped before consuming frame",
        ));
    }
    Ok(())
}

pub struct ActorRemoteClient {
    runtime: tokio::runtime::Runtime,
    endpoint: quinn::Endpoint,
    connection: quinn::Connection,
    lease: MachineLinkLease,
    session: String,
    branch: String,
}
impl ActorRemoteClient {
    pub fn connect(
        store: &SessionStore,
        state_dir: &Path,
        opts: MachineLinkConnectOptions,
        session: &str,
        branch: &str,
    ) -> Result<Self> {
        validate_caps(&opts.requested)?;
        validate_lease(opts.lease)?;
        if opts.requested.iter().any(|cap| !is_actor_capability(*cap)) {
            return Err(Error::Denied(
                "Actor remote client may request only named Actor capabilities",
            ));
        }
        let peer = store.awnet_peer_by_node_id(&opts.route.peer_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied("remote Actor peer trust revoked"));
        }
        let identity = ensure_identity(store, state_dir)?;
        let link = open_punched_quic(store, state_dir, opts.route)?;
        let lease = link.runtime.block_on(negotiate_client(
            &link.connection,
            &identity,
            &peer,
            &opts.requested,
            opts.lease,
        ))?;
        Ok(Self {
            runtime: link.runtime,
            endpoint: link.endpoint,
            connection: link.connection,
            lease,
            session: session.into(),
            branch: branch.into(),
        })
    }
    pub fn request(&self, request: ActorRemoteRequest) -> Result<ActorRemoteResult> {
        self.runtime.block_on(self.request_internal(request))
    }
    /// Two request streams over ONE signed MachineLink lease. This is the
    /// documented concurrent active-Turn Control path; no second Agent or
    /// network connection is spun up by the caller. The first step may have
    /// performed paid effects before Control; the result is not retryable.
    pub fn step_and_control(
        &self,
        control: ActorRemoteRequest,
        delay_ms: u64,
    ) -> Result<(ActorRemoteResult, ActorRemoteResult)> {
        if !(1..=10_000).contains(&delay_ms)
            || !matches!(control, ActorRemoteRequest::Control { .. })
            || !self
                .lease
                .capabilities
                .contains(&MachineCapability::ActorControl)
            || !self
                .lease
                .capabilities
                .contains(&MachineCapability::ActorStep)
        {
            return Err(Error::Denied(
                "concurrent Actor Control needs signed Step+Control lease and bounded delay",
            ));
        }
        self.runtime.block_on(async {
            let step = async { self.request_internal(ActorRemoteRequest::Step {}).await };
            let control_fut = async {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                self.request_internal(control).await
            };
            let (step, control) = tokio::join!(step, control_fut);
            Ok((step?, control?))
        })
    }
    async fn request_internal(&self, request: ActorRemoteRequest) -> Result<ActorRemoteResult> {
        request.validate()?;
        if !self.lease.capabilities.contains(&request.capability()) {
            return Err(Error::Denied(
                "signed Actor lease does not allow request class",
            ));
        }
        let remain = self.lease.expires_at.saturating_sub(now_secs());
        if remain == 0 {
            return Err(Error::Denied("signed Actor lease expired"));
        }
        let id = new_request_id()?;
        let envelope = WireRequest {
            version: VERSION,
            lease_id: self.lease.lease_id.clone(),
            request_id: id.clone(),
            session_id: self.session.clone(),
            branch_id: self.branch.clone(),
            request,
        };
        tokio::time::timeout(Duration::from_secs(remain), async {
            let (mut send, mut recv) = self
                .connection
                .open_bi()
                .await
                .map_err(|_| Error::Transport("cannot open signed Actor request stream"))?;
            write_actor_frame(&mut send, &envelope, MAX_REQUEST).await?;
            let reply: WireResponse = read_actor_frame(&mut recv, MAX_REPLY).await?;
            if reply.version != VERSION
                || reply.lease_id != self.lease.lease_id
                || reply.request_id != id
            {
                return Err(Error::Protocol("signed Actor result correlation failed"));
            }
            Ok(ActorRemoteResult {
                lease: self.lease.clone(),
                status: reply.status,
                result: reply.result,
                code: reply.code,
                needs_inspection: reply.needs_inspection,
            })
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub fn close(self) {
        self.connection
            .close(0u32.into(), b"Actor peer transaction complete");
        self.endpoint
            .close(0u32.into(), b"Actor peer transaction complete");
        self.runtime.block_on(self.endpoint.wait_idle());
    }
}

#[derive(Clone)]
pub struct ActorRemoteScope {
    pub store: SessionStore,
    pub owner: PrincipalId,
    pub session: String,
    pub branch: String,
    pub site: String,
    pub writer_epoch: u64,
    pub peer_node_id: String,
    pub peer_security_epoch: u64,
}
impl ActorRemoteScope {
    pub fn from_service(
        store: SessionStore,
        service: &Service,
        peer_node_id: &str,
        peer_security_epoch: u64,
    ) -> Self {
        let (owner, session, branch, site, epoch) = service.bound_peer_scope();
        Self {
            store,
            owner: owner.clone(),
            session: session.into(),
            branch: branch.into(),
            site: site.into(),
            writer_epoch: epoch,
            peer_node_id: peer_node_id.into(),
            peer_security_epoch,
        }
    }
    /// A previously signed MachineLink identity is never sufficient:
    /// recheck the owner's current revocable grant on every operation.
    pub fn authorize(&self, permission: ActorPeerPermission) -> Result<()> {
        self.store.authorize_actor_peer(
            &self.owner,
            &self.session,
            &self.branch,
            &self.site,
            &self.peer_node_id,
            self.writer_epoch,
            self.peer_security_epoch,
            permission,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct ActorExecution {
    scope: ActorRemoteScope,
    service: Arc<Service>,
    agent: Arc<AsyncMutex<Agent>>,
    current_cancel: Arc<Mutex<Option<CancellationToken>>>,
    signed_lease_expires: u64,
}
impl ActorExecution {
    async fn perform(&self, request: ActorRemoteRequest) -> Result<Value> {
        if self.signed_lease_expires <= now_secs() {
            return Err(Error::Denied("Actor signed lease expired before delivery"));
        }
        self.scope.authorize(request.permission())?;
        self.service.verify_rpc_state()?;
        request.validate()?;
        match request {
            ActorRemoteRequest::Query { operation } => {
                let op = ActorOperation::from_rpc_value(operation)?;
                if !matches!(
                    op,
                    ActorOperation::QueryInput { .. }
                        | ActorOperation::QueryControl { .. }
                        | ActorOperation::ListControls
                        | ActorOperation::QueryQueue { .. }
                        | ActorOperation::ListQueue
                        | ActorOperation::QueryNotice { .. }
                        | ActorOperation::ListNotices
                ) {
                    return Err(Error::Denied("remote Actor query cannot mutate state"));
                }
                Ok(public_operation(self.service.operate_bound(op)?))
            }
            ActorRemoteRequest::Inspect { operation } => {
                let op = ActorOperation::from_rpc_value(operation)?;
                if !matches!(
                    op,
                    ActorOperation::InspectInput { .. }
                        | ActorOperation::InspectControl { .. }
                        | ActorOperation::DeleteQueue { .. }
                        | ActorOperation::InspectQueue { .. }
                        | ActorOperation::AckNotice { .. }
                        | ActorOperation::InspectNotice { .. }
                ) {
                    return Err(Error::Denied(
                        "remote Actor operation must be an explicit disposition",
                    ));
                }
                Ok(public_operation(self.service.operate_bound(op)?))
            }
            ActorRemoteRequest::Enqueue {
                request_id,
                message_id,
                text,
                priority,
                target_turn,
            } => {
                let agent = self
                    .agent
                    .try_lock()
                    .map_err(|_| Error::Conflict("original Agent is already dispatching"))?;
                let target = target_turn.map_or(QueueTarget::Auto, QueueTarget::Exact);
                Ok(
                    json!({"kind":"queue","fact":self.service.enqueue_bound_text(&agent,
                    &request_id,message_id,target,priority,&text)?}),
                )
            }
            ActorRemoteRequest::Step {} => {
                let mut agent = self
                    .agent
                    .try_lock()
                    .map_err(|_| Error::Conflict("Actor already running a journaled Turn"))?;
                let Some(_) = self.service.admit_next_queued(&agent)? else {
                    return Ok(json!({"kind":"idle","model_called":false}));
                };
                let cancellation = CancellationToken::new();
                {
                    let mut guard = self
                        .current_cancel
                        .lock()
                        .map_err(|_| Error::Storage("Actor cancel lock poisoned"))?;
                    *guard = Some(cancellation.clone());
                }
                // This is the SAME original Agent/ToolPolicy/TurnJournal.
                // Revocation during an in-flight attempt drops the future;
                // ambiguous paid work stays in the immutable Unknown ledger.
                let result = async {
                    let running = self.service.run_once(&mut agent, |_| Ok(()));
                    tokio::pin!(running);
                    let mut ticker = tokio::time::interval(Duration::from_millis(150));
                    loop {
                        tokio::select! {
                            outcome=&mut running=>break outcome,
                            _=cancellation.cancelled()=>break Err(Error::Cancelled),
                            _=ticker.tick()=>{
                                if self.signed_lease_expires <= now_secs() {
                                    return Err(Error::Denied("Actor signed lease expired during original Turn"));
                                }
                                self.scope.authorize(ActorPeerPermission::Step)?;
                                self.service.verify_rpc_state()?;
                            },
                        }
                    }
                }
                .await;
                *self
                    .current_cancel
                    .lock()
                    .map_err(|_| Error::Storage("Actor cancel lock poisoned"))? = None;
                match result? {
                    Some(turn) => Ok(json!({"kind":"turn","id":turn.id,"outcome":turn.outcome,
                        "answer":turn.answer,"model":turn.model,"profile":turn.profile,
                        "needs_reconciliation":turn.needs_reconciliation})),
                    None => Err(Error::Conflict(
                        "original Actor never produced one exact settled Turn",
                    )),
                }
            }
            ActorRemoteRequest::Control {
                request_id,
                message_id,
                action,
            } => {
                let fact = self.service.control(request_id, action, message_id)?;
                if action == ActorControlAction::CancelTurn
                    && fact.state == ActorControlState::Applied
                {
                    if let Ok(current) = self.current_cancel.lock() {
                        if let Some(token) = current.as_ref() {
                            token.cancel();
                        }
                    }
                }
                Ok(public_operation(ActorOperationResult::Control { fact }))
            }
            ActorRemoteRequest::NoticeRead { request_id } => {
                let body = self.service.notice_private(&request_id)?;
                Ok(json!({"kind":"notice_private","request_id":request_id,"text":body.as_str()}))
            }
        }
    }
}

async fn one_request(
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    peer: ActorExecution,
    lease: MachineLinkLease,
    seen: Arc<Mutex<BTreeSet<String>>>,
) -> Result<()> {
    let wire: WireRequest = read_actor_frame(&mut recv, MAX_REQUEST).await?;
    if wire.version != VERSION
        || wire.lease_id != lease.lease_id
        || wire.session_id != peer.scope.session
        || wire.branch_id != peer.scope.branch
        || wire.request_id.is_empty()
        || wire.request_id.len() > 64
        || !wire
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Denied(
            "Actor QUIC request not bound to signed lease/Session/branch",
        ));
    }
    if lease.expires_at <= now_secs() || !lease.capabilities.contains(&wire.request.capability()) {
        return Err(Error::Denied(
            "Actor signed capability lease expired or does not grant operation",
        ));
    }
    {
        let mut guard = seen
            .lock()
            .map_err(|_| Error::Storage("Actor peer request lock poisoned"))?;
        if guard.len() >= MAX_FRAMES || !guard.insert(wire.request_id.clone()) {
            return Err(Error::Conflict(
                "Actor remote request ID replay or bounded limit exceeded",
            ));
        }
    }
    let requested_permission = wire.request.permission();
    let data_only = matches!(
        wire.request,
        ActorRemoteRequest::Query { .. } | ActorRemoteRequest::NoticeRead { .. }
    );
    let result = peer.perform(wire.request).await;
    // Never send private data to a peer whose Session grant was revoked
    // after execution but before the response. An effect may already have
    // happened; that still requires reconciliation, not a retry.
    let result = match result {
        Ok(value) if lease.expires_at > now_secs() => {
            peer.scope.authorize(requested_permission).map(|_| value)
        }
        Ok(_) => Err(Error::Denied(
            "Actor signed lease expired before returning private result",
        )),
        Err(error) => Err(error),
    };
    let (status, body, code, needs_inspection) = match result {
        Ok(value) => ("settled".into(), Some(value), None, false),
        Err(error) => (
            if data_only {
                "rejected"
            } else {
                "unknown_or_rejected"
            }
            .into(),
            None,
            Some(aw::headless::error_code(&error).into()),
            !data_only,
        ),
    };
    let reply = WireResponse {
        version: VERSION,
        lease_id: lease.lease_id,
        request_id: wire.request_id,
        status,
        result: body,
        code,
        needs_inspection,
    };
    write_actor_frame(&mut send, &reply, MAX_REPLY).await
}

/// The existing exclusive owner.lock prevents another CLI from reopening the
/// active owner's store. Minimal typed live revocation is accepted only from
/// the explicitly launched server's LOCAL stdin, never from QUIC or model text.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ActorLocalAdmin {
    Revoke { grant_id: String },
}

fn read_admin_line<R: BufRead>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    const MAX: usize = 2048;
    let mut bytes = Vec::new();
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok((!bytes.is_empty()).then_some(bytes));
        }
        let size = buffer
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buffer.len(), |n| n + 1);
        let ended = buffer[size - 1] == b'\n';
        if bytes.len().saturating_add(size) > MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "local admin frame too large",
            ));
        }
        bytes.extend_from_slice(&buffer[..size]);
        reader.consume(size);
        if ended {
            return Ok(Some(bytes));
        }
    }
}

fn local_admin_stdin(store: SessionStore, owner: PrincipalId, session: String, epoch: u64) {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    for _ in 0..128 {
        let bytes = match read_admin_line(&mut reader) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => break,
            Err(_) => {
                eprintln!("aw actor-owner: oversized local admin command");
                break;
            }
        };
        match serde_json::from_slice::<ActorLocalAdmin>(&bytes) {
            Ok(ActorLocalAdmin::Revoke { grant_id })
                if !grant_id.is_empty()
                    && grant_id.len() <= 128
                    && grant_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)) =>
            {
                match store.revoke_actor_peer(&owner, &session, &grant_id, epoch) {
                    Ok(_) => eprintln!("aw actor-owner: peer grant revoked"),
                    Err(_) => eprintln!(
                        "aw actor-owner: revocation rejected by current writer or grant ledger"
                    ),
                }
            }
            _ => eprintln!("aw actor-owner: invalid typed local admin command"),
        }
    }
}

/// Serve only one explicitly paired and writer-granted machine peer for a
/// bounded signed MachineLink lease. Every operation rechecks current Owner,
/// branch, Actor generation and revocable peer grant; it cannot register a
/// second model runner or resurrect uncertain Input after a lost connection.
pub fn serve_actor_remote(
    store: SessionStore,
    state_dir: &Path,
    opts: MachineLinkServeOptions,
    service: Service,
    agent: Agent,
) -> Result<ActorRemoteServed> {
    validate_caps(&opts.allowed)?;
    validate_lease(opts.lease)?;
    if opts.allowed.iter().any(|cap| !is_actor_capability(*cap)) {
        return Err(Error::Denied(
            "remote Actor may grant only explicit Actor capabilities",
        ));
    }
    service.verify_rpc_agent(&agent)?;
    service.verify_rpc_state()?;
    let peer = store.awnet_peer_by_node_id(&opts.route.peer_node_id)?;
    if peer.trust_state != "trusted" {
        return Err(Error::Denied("remote Actor peer trust was revoked"));
    }
    let identity = ensure_identity(&store, state_dir)?;
    let link = open_punched_quic(&store, state_dir, opts.route)?;
    let (owner, session, _, _, epoch) = service.bound_peer_scope();
    let local_store = store.clone();
    let local_owner = owner.clone();
    let local_session = session.to_owned();
    std::thread::Builder::new()
        .name("aw-actor-local-admin".into())
        .spawn(move || local_admin_stdin(local_store, local_owner, local_session, epoch))
        .map_err(|_| Error::Storage("cannot start local Actor revocation reader"))?;
    let result=link.runtime.block_on(async {
        let lease=negotiate_server(&link.connection,&identity,&peer,&opts.allowed,opts.lease).await?;
        let scope=ActorRemoteScope::from_service(store,&service,&peer.node_id,peer.security_epoch);
        let engine=ActorExecution{
            scope, service:Arc::new(service),agent:Arc::new(AsyncMutex::new(agent)),
            current_cancel:Arc::new(Mutex::new(None)),
            signed_lease_expires: lease.expires_at,
        };
        let seen=Arc::new(Mutex::new(BTreeSet::new()));
        let slots=Arc::new(Semaphore::new(MAX_CONCURRENT));
        let mut tasks=JoinSet::new();
        let mut count=0usize;
        while count<MAX_FRAMES {
            let remaining=lease.expires_at.saturating_sub(now_secs());
            if remaining==0 {break;}
            let accepted=tokio::select! {
                _=tasks.join_next(), if !tasks.is_empty()=>continue,
                response=tokio::time::timeout(Duration::from_secs(remaining),link.connection.accept_bi())=>response,
            };
            let (send,recv)=match accepted {
                Err(_)=>break,
                Ok(Ok(streams))=>streams,
                Ok(Err(_)) if link.connection.close_reason().is_some()=>break,
                Ok(Err(_))=>return Err(Error::Transport("Actor QUIC accept stream failed")),
            };
            let Ok(slot)=slots.clone().try_acquire_owned() else {
                // No implicit queueing of a ninth concurrent remote action.
                drop(send);drop(recv);continue;
            };
            let shared=engine.clone();
            let signed_lease=lease.clone();
            let seen=seen.clone();
            tasks.spawn(async move {
                let _slot=slot;
                one_request(send,recv,shared,signed_lease,seen).await
            });
            count+=1;
        }
        // Release the one original Agent only once all live streams have
        // stopped; cancelled/uncertain work is never silently retried.
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        Ok::<_,Error>(ActorRemoteServed{lease,handled:count})
    });
    link.connection
        .close(0u32.into(), b"Actor signed lease finished");
    link.endpoint
        .close(0u32.into(), b"Actor signed lease finished");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_9c2_typed_remote_capabilities_do_not_infer_other_grants() {
        for (request, capability) in [
            (
                ActorRemoteRequest::Query {
                    operation: json!({"op":"list_queue"}),
                },
                MachineCapability::ActorQuery,
            ),
            (ActorRemoteRequest::Step {}, MachineCapability::ActorStep),
            (
                ActorRemoteRequest::Control {
                    request_id: "ctrl".into(),
                    message_id: 3,
                    action: ActorControlAction::CancelTurn,
                },
                MachineCapability::ActorControl,
            ),
        ] {
            assert_eq!(request.capability(), capability);
            request.validate().unwrap();
        }
        assert!(!is_actor_capability(MachineCapability::FileRead));
        assert_eq!(
            MachineCapability::parse("actor-step").unwrap(),
            MachineCapability::ActorStep
        );
        assert_eq!(
            MachineCapability::parse("actor-control").unwrap(),
            MachineCapability::ActorControl
        );
        assert!(
            serde_json::from_value::<ActorRemoteRequest>(json!({
                "op":"step","actor_key":"forged"
            }))
            .is_err(),
            "strict empty Step rejects caller-supplied Actor authority"
        );
        assert!(
            serde_json::from_value::<ActorLocalAdmin>(json!({
                "op":"revoke","grant_id":"apg_123","actor_key":"forged"
            }))
            .is_err(),
            "local admin commands reject unapproved authority fields"
        );
        assert!(
            read_admin_line(&mut std::io::Cursor::new(vec![b'a'; 2049])).is_err(),
            "oversized owner-admin input is rejected before JSON parse"
        );
        assert!(ActorRemoteRequest::Query {
            operation: json!({
                "op":"list_queue","actor_key":"forged"
            })
        }
        .validate()
        .is_err());
        assert!(serde_json::from_value::<ActorRemoteRequest>(json!({
            "op":"control","request_id":"ctrl","message_id":4,
            "action":"cancel_turn","admin":true
        }))
        .is_err());
        assert!(ActorRemoteRequest::Enqueue {
            request_id: "id".into(),
            message_id: 1,
            text: "x".into(),
            priority: 19,
            target_turn: Some(0)
        }
        .validate()
        .is_err());
    }
}
