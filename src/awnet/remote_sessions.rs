use super::machinelink::{
    negotiate_client, negotiate_server, MachineCapability, MachineLinkConnectOptions,
    MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_store::{RemoteSessionRef, SessionBinding, WorkspaceInfo};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

const REMOTE_SESSION_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteSessionIntent {
    operation_id: String,
    source_store_id: String,
    source_domain_id: String,
    source_workspace_id: String,
    source_workspace_name: String,
    source_node_id: String,
    source_session_id: Option<String>,
    source_branch_id: Option<String>,
    title: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteSessionRequest {
    version: u16,
    lease_id: String,
    request_id: String,
    request_sha256: String,
    intent: RemoteSessionIntent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteSessionResponse {
    version: u16,
    lease_id: String,
    request_id: String,
    operation_id: String,
    request_sha256: String,
    remote_store_id: String,
    remote_workspace_id: String,
    remote_workspace_name: String,
    remote_session_id: String,
    remote_branch_id: String,
    reused: bool,
}

#[derive(Clone, Debug)]
pub struct RemoteSessionServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
    pub root: PathBuf,
    pub drop_response_after_create: bool,
}

#[derive(Clone, Debug)]
pub struct RemoteSessionCreateOptions {
    pub link: MachineLinkConnectOptions,
    pub actor: PrincipalId,
    pub source_workspace: WorkspaceInfo,
    pub source_session_id: Option<String>,
    pub source_branch_id: Option<String>,
    pub title: String,
    pub operation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteSessionCreated {
    pub remote_store_id: String,
    pub remote_workspace_id: String,
    pub remote_workspace_name: Option<String>,
    pub remote_session_id: String,
    pub remote_branch_id: String,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteSessionCreateResult {
    pub reference: RemoteSessionRef,
    pub remote: RemoteSessionCreated,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteSessionServeResult {
    pub lease: MachineLinkLease,
    pub operation_id: String,
    pub request_sha256: String,
    pub remote: RemoteSessionCreated,
}

fn random_operation_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for remote Session operation"))?;
    Ok(format!("ro_{}", hex(&bytes)))
}

fn remote_request_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for remote Session request"))?;
    Ok(format!("rsq_{}", hex(&bytes)))
}

fn intent_digest(intent: &RemoteSessionIntent) -> Result<String> {
    let encoded = serde_json::to_vec(intent)
        .map_err(|_| Error::Protocol("cannot encode remote Session immutable intent"))?;
    Ok(hex(&Sha256::digest(encoded)))
}

fn remote_binding(root: &Path) -> Result<SessionBinding> {
    let root = root
        .canonicalize()
        .map_err(|_| Error::Config("remote Session root must be an existing directory".into()))?;
    if !root.is_dir() {
        return Err(Error::Config(
            "remote Session root must be an existing directory".into(),
        ));
    }
    Ok(SessionBinding {
        working_directory: root.clone(),
        tool_root: root,
        system_prompt: String::new(),
        profile_hint: "remote-delegated".into(),
        model_hint: "unassigned".into(),
        destination_fingerprint: hex(&Sha256::digest(b"aw.remote-session.unconfigured.v1")),
    })
}

fn from_active_ref(reference: RemoteSessionRef) -> Result<RemoteSessionCreateResult> {
    let remote_store_id = reference.remote_store_id.clone().ok_or(Error::Storage(
        "active RemoteSessionRef missing remote store",
    ))?;
    let remote_workspace_id = reference.remote_workspace_id.clone().ok_or(Error::Storage(
        "active RemoteSessionRef missing remote Workspace",
    ))?;
    let remote_session_id = reference.remote_session_id.clone().ok_or(Error::Storage(
        "active RemoteSessionRef missing remote Session",
    ))?;
    let remote_branch_id = reference.remote_branch_id.clone().ok_or(Error::Storage(
        "active RemoteSessionRef missing remote branch",
    ))?;
    Ok(RemoteSessionCreateResult {
        reference,
        remote: RemoteSessionCreated {
            remote_store_id,
            remote_workspace_id,
            remote_workspace_name: None,
            remote_session_id,
            remote_branch_id,
            reused: true,
        },
    })
}

pub fn serve_remote_session(
    store: &SessionStore,
    state_dir: &Path,
    opts: RemoteSessionServeOptions,
) -> Result<RemoteSessionServeResult> {
    if !opts
        .link
        .allowed
        .contains(&MachineCapability::RemoteSession)
    {
        return Err(Error::Config(
            "remote Session server must explicitly grant remote-session".into(),
        ));
    }
    let binding = remote_binding(&opts.root)?;
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
        if !lease.capabilities.contains(&MachineCapability::RemoteSession) {
            return Err(Error::Denied(
                "remote-session capability was not negotiated",
            ));
        }
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("remote Session request stream was not opened"))?;
        let request: RemoteSessionRequest = read_frame(&mut recv).await?;
        if request.version != REMOTE_SESSION_VERSION || request.lease_id != lease.lease_id {
            return Err(Error::Protocol("remote Session lease/request mismatch"));
        }
        if request.intent.source_node_id != peer.node_id {
            return Err(Error::Denied(
                "remote Session source Node ID differs from the authenticated peer",
            ));
        }
        let digest = intent_digest(&request.intent)?;
        if digest != request.request_sha256 {
            return Err(Error::Protocol(
                "remote Session immutable intent digest mismatch",
            ));
        }
        let placement = store.create_peer_session_idempotent(
            &opts.actor,
            &request.intent.source_store_id,
            &request.intent.source_domain_id,
            &request.intent.source_workspace_id,
            &request.intent.source_workspace_name,
            &request.intent.source_node_id,
            request.intent.source_session_id.as_deref(),
            request.intent.source_branch_id.as_deref(),
            &request.intent.operation_id,
            &request.request_sha256,
            &request.intent.title,
            binding,
        )?;
        let remote_store_id = store.store_id()?;
        let remote = RemoteSessionCreated {
            remote_store_id: remote_store_id.clone(),
            remote_workspace_id: placement.workspace.id.clone(),
            remote_workspace_name: Some(placement.workspace.name.clone()),
            remote_session_id: placement.session.id.clone(),
            remote_branch_id: placement.branch.id.clone(),
            reused: placement.reused,
        };
        if opts.drop_response_after_create {
            return Err(Error::Transport(
                "validation fault: remote Session response intentionally dropped after durable create",
            ));
        }
        write_frame(
            &mut send,
            &RemoteSessionResponse {
                version: REMOTE_SESSION_VERSION,
                lease_id: lease.lease_id.clone(),
                request_id: request.request_id,
                operation_id: request.intent.operation_id.clone(),
                request_sha256: request.request_sha256.clone(),
                remote_store_id,
                remote_workspace_id: placement.workspace.id,
                remote_workspace_name: placement.workspace.name,
                remote_session_id: placement.session.id,
                remote_branch_id: placement.branch.id,
                reused: placement.reused,
            },
        )
        .await?;
        Ok::<_, Error>(RemoteSessionServeResult {
            lease,
            operation_id: request.intent.operation_id,
            request_sha256: request.request_sha256,
            remote,
        })
    });
    link.connection
        .close(0u32.into(), b"remote Session operation complete");
    link.endpoint
        .close(0u32.into(), b"remote Session operation complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn create_remote_session(
    store: &SessionStore,
    state_dir: &Path,
    opts: RemoteSessionCreateOptions,
) -> Result<RemoteSessionCreateResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::RemoteSession)
    {
        return Err(Error::Config(
            "remote Session client must explicitly request remote-session".into(),
        ));
    }
    if opts.source_session_id.is_some() != opts.source_branch_id.is_some() {
        return Err(Error::Config(
            "source Session and branch provenance must be supplied together".into(),
        ));
    }
    if let (Some(session_id), Some(branch_id)) = (
        opts.source_session_id.as_deref(),
        opts.source_branch_id.as_deref(),
    ) {
        let session = store.session(&opts.actor, session_id)?;
        if session.workspace_id != opts.source_workspace.id {
            return Err(Error::Denied(
                "source Session is outside the selected source Workspace",
            ));
        }
        let branches = store.branches(&opts.actor, session_id)?;
        if !branches
            .iter()
            .any(|branch| branch.id == branch_id || branch.name == branch_id)
        {
            return Err(Error::Denied("source branch unavailable"));
        }
    }
    let operation_id = opts.operation_id.unwrap_or(random_operation_id()?);
    let source_store_id = store.store_id()?;
    let local = store.local_machine()?;
    let intent = RemoteSessionIntent {
        operation_id: operation_id.clone(),
        source_store_id,
        source_domain_id: opts.source_workspace.domain_id.clone(),
        source_workspace_id: opts.source_workspace.id.clone(),
        source_workspace_name: opts.source_workspace.name.clone(),
        source_node_id: local.node_id,
        source_session_id: opts.source_session_id.clone(),
        source_branch_id: opts.source_branch_id.clone(),
        title: opts.title,
    };
    let request_sha256 = intent_digest(&intent)?;
    let reference = store.begin_remote_session_ref(
        &opts.actor,
        &operation_id,
        &request_sha256,
        &opts.source_workspace.id,
        opts.source_session_id.as_deref(),
        &opts.link.route.peer_node_id,
    )?;
    if reference.state == "active" {
        return from_active_ref(reference);
    }

    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let link = open_punched_quic(store, state_dir, opts.link.route)?;
    let lease = link.runtime.block_on(negotiate_client(
        &link.connection,
        &identity,
        &peer,
        &opts.link.requested,
        opts.link.lease,
    ))?;
    if !lease
        .capabilities
        .contains(&MachineCapability::RemoteSession)
    {
        return Err(Error::Denied(
            "remote peer did not grant remote-session capability",
        ));
    }
    let (mut send, mut recv) = link.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("cannot open remote Session request stream"))
    })?;
    let request_id = remote_request_id()?;
    let request = RemoteSessionRequest {
        version: REMOTE_SESSION_VERSION,
        lease_id: lease.lease_id.clone(),
        request_id: request_id.clone(),
        request_sha256: request_sha256.clone(),
        intent,
    };

    // From this point onward the remote durable effect may have happened.
    let _ = store.mark_remote_session_ref_unknown(&opts.actor, &operation_id)?;
    let sent = link.runtime.block_on(write_frame(&mut send, &request));
    if let Err(error) = sent {
        link.connection
            .close(1u32.into(), b"remote Session dispatch uncertain");
        link.endpoint
            .close(1u32.into(), b"remote Session dispatch uncertain");
        link.runtime.block_on(link.endpoint.wait_idle());
        return Err(error);
    }
    let response: RemoteSessionResponse = match link.runtime.block_on(read_frame(&mut recv)) {
        Ok(response) => response,
        Err(error) => {
            link.connection
                .close(1u32.into(), b"remote Session response unknown");
            link.endpoint
                .close(1u32.into(), b"remote Session response unknown");
            link.runtime.block_on(link.endpoint.wait_idle());
            return Err(error);
        }
    };
    if response.version != REMOTE_SESSION_VERSION
        || response.lease_id != lease.lease_id
        || response.request_id != request_id
        || response.operation_id != operation_id
        || response.request_sha256 != request_sha256
    {
        return Err(Error::Protocol(
            "remote Session response correlation failed; inspect unknown operation before retry",
        ));
    }
    let reference = store.resolve_remote_session_ref(
        &opts.actor,
        &operation_id,
        &response.remote_store_id,
        &response.remote_workspace_id,
        &response.remote_session_id,
        &response.remote_branch_id,
    )?;
    let result = RemoteSessionCreateResult {
        reference,
        remote: RemoteSessionCreated {
            remote_store_id: response.remote_store_id,
            remote_workspace_id: response.remote_workspace_id,
            remote_workspace_name: Some(response.remote_workspace_name),
            remote_session_id: response.remote_session_id,
            remote_branch_id: response.remote_branch_id,
            reused: response.reused,
        },
    };
    link.connection
        .close(0u32.into(), b"remote Session resolved");
    link.endpoint.close(0u32.into(), b"remote Session resolved");
    link.runtime.block_on(link.endpoint.wait_idle());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_binding_is_unconfigured_for_inference() {
        let root = std::env::temp_dir();
        let binding = remote_binding(&root).unwrap();
        assert_eq!(binding.profile_hint, "remote-delegated");
        assert_eq!(binding.model_hint, "unassigned");
        assert_eq!(binding.destination_fingerprint.len(), 64);
    }
}
