use super::machinelink::{
    negotiate_client, validate_caps, validate_lease, MachineCapability, MachineLinkConnectOptions,
    MachineLinkLease,
};
#[cfg(feature = "web")]
use super::machinelink::{negotiate_server, MachineLinkServeOptions};
use super::reachability::open_punched_quic;
use super::*;
#[cfg(feature = "web")]
use aw::PrincipalId;
use aw_client::Receipt;
#[cfg(feature = "web")]
use aw_client::{Client, Command, PromptInput};
use aw_store::RemoteSessionExecutionInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(feature = "web")]
use std::path::PathBuf;
use std::{path::Path, time::Duration};

const REMOTE_CONTROL_VERSION: u16 = 1;
#[cfg(feature = "web")]
const MAX_REMOTE_CONTROL_REQUESTS_PER_LEASE: u64 = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteControlRequest {
    Execute {
        session_id: String,
        branch_id: String,
        execution_id: String,
        prompt: String,
    },
    Status {
        session_id: String,
        branch_id: String,
        execution_id: Option<String>,
    },
    Transcript {
        session_id: String,
        branch_id: String,
    },
    Cancel {
        session_id: String,
        branch_id: String,
        run_id: u64,
        execution_id: Option<String>,
    },
}

impl RemoteControlRequest {
    pub fn required_capability(&self) -> MachineCapability {
        match self {
            Self::Execute { .. } => MachineCapability::RemoteSessionExecute,
            Self::Status { .. } => MachineCapability::RemoteSessionStatus,
            Self::Transcript { .. } => MachineCapability::RemoteSessionTranscript,
            Self::Cancel { .. } => MachineCapability::RemoteSessionCancel,
        }
    }

    #[cfg(feature = "web")]
    fn target(&self) -> (&str, &str) {
        match self {
            Self::Execute {
                session_id,
                branch_id,
                ..
            }
            | Self::Status {
                session_id,
                branch_id,
                ..
            }
            | Self::Transcript {
                session_id,
                branch_id,
            }
            | Self::Cancel {
                session_id,
                branch_id,
                ..
            } => (session_id, branch_id),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteControlResponse {
    Execution {
        receipt: Option<Receipt>,
        execution: RemoteSessionExecutionInfo,
        found_only: bool,
    },
    Status {
        revision: u64,
        pending_run_id: Option<u64>,
        needs_reconciliation: bool,
        last_error: Option<String>,
        execution: Option<RemoteSessionExecutionInfo>,
        receipt_status: Option<Value>,
    },
    Transcript {
        history: Value,
    },
    Cancelled {
        run_id: u64,
        execution: Option<RemoteSessionExecutionInfo>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteControlEnvelope {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    request: RemoteControlRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteControlReply {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    response: RemoteControlResponse,
}

#[derive(Clone, Debug)]
#[cfg(feature = "web")]
pub struct RemoteControlServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
    pub owner_socket: PathBuf,
}

#[derive(Clone, Debug)]
pub struct RemoteControlRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub request: RemoteControlRequest,
}

#[derive(Clone, Debug, Serialize)]
#[cfg(feature = "web")]
pub struct RemoteControlServeResult {
    pub lease: MachineLinkLease,
    pub requests_handled: u64,
    pub last_response: Option<RemoteControlResponse>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteControlRequestResult {
    pub lease: MachineLinkLease,
    pub response: RemoteControlResponse,
}

pub struct RemoteControlConnection {
    runtime: tokio::runtime::Runtime,
    endpoint: quinn::Endpoint,
    connection: quinn::Connection,
    lease: MachineLinkLease,
}

impl RemoteControlConnection {
    pub fn connect(
        store: &SessionStore,
        state_dir: &Path,
        opts: MachineLinkConnectOptions,
    ) -> Result<Self> {
        validate_caps(&opts.requested)?;
        validate_lease(opts.lease)?;
        if !opts.requested.iter().any(|cap| {
            matches!(
                cap,
                MachineCapability::RemoteSessionExecute
                    | MachineCapability::RemoteSessionStatus
                    | MachineCapability::RemoteSessionTranscript
                    | MachineCapability::RemoteSessionCancel
            )
        }) {
            return Err(Error::Config(
                "remote control client must explicitly request a Session control capability".into(),
            ));
        }
        let peer = store.awnet_peer_by_node_id(&opts.route.peer_node_id)?;
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
        })
    }

    pub fn request(&mut self, request: RemoteControlRequest) -> Result<RemoteControlRequestResult> {
        let required = request.required_capability();
        ensure_capability(&self.lease.capabilities, required)?;
        let remaining = self.lease.expires_at.saturating_sub(now_secs());
        if remaining == 0 {
            return Err(Error::Denied("MachineLink remote control lease expired"));
        }
        let connection = self.connection.clone();
        let lease_id = self.lease.lease_id.clone();
        let lease = self.lease.clone();
        self.runtime.block_on(async move {
            let exchange = async {
                let (mut send, mut recv) = connection
                    .open_bi()
                    .await
                    .map_err(|_| Error::Transport("cannot open remote Session control stream"))?;
                let request_id = request_id()?;
                write_data_frame(
                    &mut send,
                    &RemoteControlEnvelope {
                        version: REMOTE_CONTROL_VERSION,
                        machine_lease_id: lease_id.clone(),
                        request_id: request_id.clone(),
                        request,
                    },
                )
                .await?;
                let reply: RemoteControlReply = read_data_frame(&mut recv).await?;
                if reply.version != REMOTE_CONTROL_VERSION
                    || reply.machine_lease_id != lease_id
                    || reply.request_id != request_id
                {
                    return Err(Error::Protocol(
                        "remote Session control response correlation failed; inspect before retry",
                    ));
                }
                Ok::<_, Error>(RemoteControlRequestResult {
                    lease,
                    response: reply.response,
                })
            };
            tokio::time::timeout(Duration::from_secs(remaining), exchange)
                .await
                .map_err(|_| Error::Timeout)?
        })
    }

    pub fn close(self) {
        let Self {
            runtime,
            endpoint,
            connection,
            ..
        } = self;
        connection.close(0u32.into(), b"remote Session control complete");
        endpoint.close(0u32.into(), b"remote Session control complete");
        runtime.block_on(endpoint.wait_idle());
    }
}

fn request_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for remote control request"))?;
    Ok(format!("rcq_{}", hex(&bytes)))
}

#[cfg(feature = "web")]
fn prompt_sha256(prompt: &str) -> String {
    hex(&Sha256::digest(prompt.as_bytes()))
}

fn ensure_capability(
    capabilities: &[MachineCapability],
    required: MachineCapability,
) -> Result<()> {
    if capabilities.contains(&required) {
        Ok(())
    } else {
        Err(Error::Denied(
            "MachineLink lease does not grant this remote Session operation",
        ))
    }
}

#[cfg(feature = "web")]
fn owner_error(_: aw_client::Error) -> Error {
    Error::Transport("local Session owner control failed; inspect durable state before retry")
}

#[cfg(feature = "web")]
async fn owner_for_target(
    socket: &Path,
    actor: &PrincipalId,
    session_id: &str,
    branch_id: &str,
) -> Result<(Client, aw_client::Snapshot)> {
    let mut client = Client::connect(socket).await.map_err(owner_error)?;
    if client.hello().principal != actor.as_str() {
        return Err(Error::Denied(
            "local owner principal does not match the remote Session authority",
        ));
    }
    let snapshot = client.snapshot().await.map_err(owner_error)?;
    let saved = snapshot
        .saved
        .as_ref()
        .ok_or(Error::Conflict("local owner has no saved Session target"))?;
    if saved.session_id != session_id || saved.branch_id != branch_id {
        return Err(Error::Conflict(
            "local owner is not currently bound to the requested remote Session/branch",
        ));
    }
    Ok((client, snapshot))
}

#[cfg(feature = "web")]
async fn receipt_lookup(client: &mut Client, input: PromptInput) -> Result<Value> {
    client
        .call(Command::PromptStatus { original: input })
        .await
        .map_err(owner_error)
}

#[cfg(feature = "web")]
#[allow(clippy::too_many_arguments)] // Explicit authority/target/idempotency fields are kept visible at this protocol boundary.
async fn execute(
    store: &SessionStore,
    actor: &PrincipalId,
    peer_node_id: &str,
    owner_socket: &Path,
    session_id: String,
    branch_id: String,
    execution_id: String,
    prompt: String,
) -> Result<RemoteControlResponse> {
    if prompt.trim().is_empty() || prompt.len() > aw_agent_core::limits::MAX_PROMPT_BYTES {
        return Err(Error::Limit("remote Session prompt exceeds bounded size"));
    }
    let digest = prompt_sha256(&prompt);
    if let Some(execution) =
        store.remote_session_execution_optional(actor, peer_node_id, &execution_id)?
    {
        if execution.session_id != session_id
            || execution.branch_id != branch_id
            || execution.prompt_sha256 != digest
        {
            return Err(Error::Conflict(
                "remote execution ID was reused for different input",
            ));
        }
        let (mut owner, _) = owner_for_target(owner_socket, actor, &session_id, &branch_id).await?;
        let original: PromptInput = serde_json::from_value(execution.owner_request.clone())
            .map_err(|_| Error::Storage("stored remote execution request is invalid"))?;
        let status = receipt_lookup(&mut owner, original).await?;
        let receipt = status
            .get("receipt")
            .cloned()
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value::<Receipt>(v).ok());
        let execution = if let Some(receipt) = &receipt {
            store.settle_remote_session_execution(
                actor,
                peer_node_id,
                &execution_id,
                receipt.run_id,
                if status.get("completion").is_some_and(|v| !v.is_null()) {
                    "settled"
                } else {
                    "active"
                },
            )?
        } else {
            execution
        };
        return Ok(RemoteControlResponse::Execution {
            receipt,
            execution,
            found_only: true,
        });
    }

    let (mut owner, snapshot) =
        owner_for_target(owner_socket, actor, &session_id, &branch_id).await?;
    let owner_input = snapshot
        .prepare(execution_id.clone(), prompt.clone(), None)
        .map_err(|_| Error::Conflict("local owner cannot currently admit this prompt"))?;
    let owner_value = serde_json::to_value(&owner_input)
        .map_err(|_| Error::Storage("cannot encode local owner request"))?;
    let (_execution, reused) = store.begin_remote_session_execution(
        actor,
        peer_node_id,
        &session_id,
        &branch_id,
        &execution_id,
        &digest,
        owner_value,
    )?;
    if reused {
        return Err(Error::Conflict(
            "remote execution appeared concurrently; retry performs lookup only",
        ));
    }

    // From this point through receipt settlement, transport/process failure can
    // leave the prompt accepted by the owner. Persist unknown BEFORE sending it.
    store.mark_remote_session_execution_unknown(actor, peer_node_id, &execution_id)?;
    let receipt: Receipt = owner
        .call(Command::Prompt(owner_input))
        .await
        .map_err(owner_error)?;
    let execution = store.settle_remote_session_execution(
        actor,
        peer_node_id,
        &execution_id,
        receipt.run_id,
        "active",
    )?;
    Ok(RemoteControlResponse::Execution {
        receipt: Some(receipt),
        execution,
        found_only: false,
    })
}

#[cfg(feature = "web")]
async fn status(
    store: &SessionStore,
    actor: &PrincipalId,
    peer_node_id: &str,
    owner_socket: &Path,
    session_id: String,
    branch_id: String,
    execution_id: Option<String>,
) -> Result<RemoteControlResponse> {
    let (mut owner, snapshot) =
        owner_for_target(owner_socket, actor, &session_id, &branch_id).await?;
    let mut execution = None;
    let mut receipt_status = None;
    if let Some(id) = execution_id {
        let current = store.remote_session_execution(actor, peer_node_id, &id)?;
        if current.session_id != session_id || current.branch_id != branch_id {
            return Err(Error::Denied(
                "remote execution does not belong to this Session/branch",
            ));
        }
        let original: PromptInput = serde_json::from_value(current.owner_request.clone())
            .map_err(|_| Error::Storage("stored remote execution request is invalid"))?;
        let found = receipt_lookup(&mut owner, original).await?;
        if let Some(receipt) = found
            .get("receipt")
            .cloned()
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value::<Receipt>(v).ok())
        {
            let settled = found.get("completion").is_some_and(|v| !v.is_null());
            execution = Some(store.settle_remote_session_execution(
                actor,
                peer_node_id,
                &id,
                receipt.run_id,
                if settled { "settled" } else { "active" },
            )?);
        } else {
            execution = Some(current);
        }
        receipt_status = Some(found);
    }
    Ok(RemoteControlResponse::Status {
        revision: snapshot.revision,
        pending_run_id: snapshot.pending.as_ref().map(|p| p.run_id),
        needs_reconciliation: snapshot.needs_reconciliation,
        last_error: snapshot.last_error,
        execution,
        receipt_status,
    })
}

#[cfg(feature = "web")]
fn transcript(
    store: &SessionStore,
    actor: &PrincipalId,
    session_id: &str,
    branch_id: &str,
) -> Result<RemoteControlResponse> {
    let (view, _) = store.activate(actor, session_id, branch_id)?;
    let history = serde_json::to_value(view.history)
        .map_err(|_| Error::Storage("cannot encode remote Session transcript"))?;
    Ok(RemoteControlResponse::Transcript { history })
}

#[cfg(feature = "web")]
#[allow(clippy::too_many_arguments)] // Cancellation binds the same explicit remote authority tuple; avoid hiding it in an opaque bag.
async fn cancel(
    store: &SessionStore,
    actor: &PrincipalId,
    peer_node_id: &str,
    owner_socket: &Path,
    session_id: String,
    branch_id: String,
    run_id: u64,
    execution_id: Option<String>,
) -> Result<RemoteControlResponse> {
    if run_id == 0 {
        return Err(Error::Config("remote cancellation run ID is zero".into()));
    }
    let (mut owner, snapshot) =
        owner_for_target(owner_socket, actor, &session_id, &branch_id).await?;
    if snapshot.pending.as_ref().map(|p| p.run_id) != Some(run_id) {
        return Err(Error::Conflict(
            "requested run is not the active run for this remote Session",
        ));
    }
    let _: Value = owner
        .call(Command::Cancel { run_id })
        .await
        .map_err(owner_error)?;
    let execution = if let Some(id) = execution_id {
        let current = store.remote_session_execution(actor, peer_node_id, &id)?;
        if current.run_id != Some(run_id)
            || current.session_id != session_id
            || current.branch_id != branch_id
        {
            return Err(Error::Conflict(
                "execution ID does not identify the cancelled remote run",
            ));
        }
        Some(store.settle_remote_session_execution(
            actor,
            peer_node_id,
            &id,
            run_id,
            "cancelled",
        )?)
    } else {
        None
    };
    Ok(RemoteControlResponse::Cancelled { run_id, execution })
}

#[cfg(feature = "web")]
async fn execute_request(
    store: &SessionStore,
    actor: &PrincipalId,
    peer_node_id: &str,
    owner_socket: &Path,
    request: RemoteControlRequest,
) -> Result<RemoteControlResponse> {
    let (session_id, _) = request.target();
    store.authorize_remote_session_peer(actor, session_id, peer_node_id)?;
    match request {
        RemoteControlRequest::Execute {
            session_id,
            branch_id,
            execution_id,
            prompt,
        } => {
            execute(
                store,
                actor,
                peer_node_id,
                owner_socket,
                session_id,
                branch_id,
                execution_id,
                prompt,
            )
            .await
        }
        RemoteControlRequest::Status {
            session_id,
            branch_id,
            execution_id,
        } => {
            status(
                store,
                actor,
                peer_node_id,
                owner_socket,
                session_id,
                branch_id,
                execution_id,
            )
            .await
        }
        RemoteControlRequest::Transcript {
            session_id,
            branch_id,
        } => transcript(store, actor, &session_id, &branch_id),
        RemoteControlRequest::Cancel {
            session_id,
            branch_id,
            run_id,
            execution_id,
        } => {
            cancel(
                store,
                actor,
                peer_node_id,
                owner_socket,
                session_id,
                branch_id,
                run_id,
                execution_id,
            )
            .await
        }
    }
}

#[cfg(feature = "web")]
pub fn serve_remote_control(
    store: &SessionStore,
    state_dir: &Path,
    opts: RemoteControlServeOptions,
) -> Result<RemoteControlServeResult> {
    validate_caps(&opts.link.allowed)?;
    validate_lease(opts.link.lease)?;
    if !opts.link.allowed.iter().any(|cap| {
        matches!(
            cap,
            MachineCapability::RemoteSessionExecute
                | MachineCapability::RemoteSessionStatus
                | MachineCapability::RemoteSessionTranscript
                | MachineCapability::RemoteSessionCancel
        )
    }) {
        return Err(Error::Config(
            "remote control server must explicitly grant a Session control capability".into(),
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let link = open_punched_quic(store, state_dir, opts.link.route)?;
    let result = link.runtime.block_on(async {
        let lease = negotiate_server(
            &link.connection,
            &identity,
            &peer,
            &opts.link.allowed,
            opts.link.lease,
        )
        .await?;
        let mut requests_handled = 0u64;
        let mut last_response = None;

        while requests_handled < MAX_REMOTE_CONTROL_REQUESTS_PER_LEASE {
            let remaining = lease.expires_at.saturating_sub(now_secs());
            if remaining == 0 {
                break;
            }
            let accepted =
                tokio::time::timeout(Duration::from_secs(remaining), link.connection.accept_bi())
                    .await;
            let (mut send, mut recv) = match accepted {
                Err(_) => break,
                Ok(Ok(streams)) => streams,
                Ok(Err(_)) if link.connection.close_reason().is_some() => break,
                Ok(Err(_)) => {
                    return Err(Error::Transport(
                        "remote Session control connection stopped accepting streams",
                    ))
                }
            };
            let envelope: RemoteControlEnvelope = read_data_frame(&mut recv).await?;
            if envelope.version != REMOTE_CONTROL_VERSION
                || envelope.machine_lease_id != lease.lease_id
            {
                return Err(Error::Protocol(
                    "remote Session control is not bound to the MachineLink lease",
                ));
            }
            ensure_capability(&lease.capabilities, envelope.request.required_capability())?;
            let response = execute_request(
                store,
                &opts.actor,
                &peer.node_id,
                &opts.owner_socket,
                envelope.request,
            )
            .await?;
            write_data_frame(
                &mut send,
                &RemoteControlReply {
                    version: REMOTE_CONTROL_VERSION,
                    machine_lease_id: lease.lease_id.clone(),
                    request_id: envelope.request_id,
                    response: response.clone(),
                },
            )
            .await?;
            requests_handled = requests_handled.saturating_add(1);
            last_response = Some(response);
        }

        Ok::<_, Error>(RemoteControlServeResult {
            lease,
            requests_handled,
            last_response,
        })
    });
    link.connection
        .close(0u32.into(), b"remote Session control complete");
    link.endpoint
        .close(0u32.into(), b"remote Session control complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_remote_control(
    store: &SessionStore,
    state_dir: &Path,
    opts: RemoteControlRequestOptions,
) -> Result<RemoteControlRequestResult> {
    let required = opts.request.required_capability();
    if !opts.link.requested.contains(&required) {
        return Err(Error::Config(
            "remote control client must explicitly request the operation capability".into(),
        ));
    }
    let mut connection = RemoteControlConnection::connect(store, state_dir, opts.link)?;
    let result = connection.request(opts.request);
    connection.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_session_controls_remain_separate_capabilities() {
        let cases = [
            (
                RemoteControlRequest::Status {
                    session_id: "s".into(),
                    branch_id: "b".into(),
                    execution_id: None,
                },
                MachineCapability::RemoteSessionStatus,
            ),
            (
                RemoteControlRequest::Transcript {
                    session_id: "s".into(),
                    branch_id: "b".into(),
                },
                MachineCapability::RemoteSessionTranscript,
            ),
            (
                RemoteControlRequest::Cancel {
                    session_id: "s".into(),
                    branch_id: "b".into(),
                    run_id: 1,
                    execution_id: None,
                },
                MachineCapability::RemoteSessionCancel,
            ),
        ];
        for (request, capability) in cases {
            assert_eq!(request.required_capability(), capability);
        }
    }
}
