use super::machinelink::{
    negotiate_client, negotiate_server, validate_caps, validate_lease, MachineCapability,
    MachineLinkConnectOptions, MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_store::{AuthenticatedStableUser, RemoteViewGrant, RemoteViewPermission};
use aw_tools_adapter::browse::{EntryKind, WorkspaceFiles};
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc, time::Duration};

const MACHINE_SERVICE_VERSION: u16 = 1;
const REMOTE_VIEW_SERVICE_VERSION: u16 = 2;
const REMOTE_WORKSPACE_SERVICE_VERSION: u16 = 3;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteWorkspaceGitMode {
    Changes,
    Worktrees,
    Branches,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteWorkspaceViewRequest {
    Files { path: String },
    FileSearch { query: String, limit: usize },
    File { path: String },
    GitList { mode: RemoteWorkspaceGitMode },
    GitOpen { inventory_id: String, item_id: String },
    Review { review_id: String },
}

impl RemoteWorkspaceViewRequest {
    pub fn permission(&self) -> RemoteViewPermission {
        match self {
            Self::Files { .. } | Self::FileSearch { .. } | Self::File { .. } => {
                RemoteViewPermission::Files
            }
            Self::GitList { .. } | Self::GitOpen { .. } | Self::Review { .. } => {
                RemoteViewPermission::Git
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteWorkspaceRequest {
    Capabilities { permission: RemoteViewPermission },
    View {
        source_id: String,
        scope_id: String,
        request: RemoteWorkspaceViewRequest,
    },
}

impl RemoteWorkspaceRequest {
    pub fn permission(&self) -> RemoteViewPermission {
        match self {
            Self::Capabilities { permission } => *permission,
            Self::View { request, .. } => request.permission(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteWorkspaceErrorCode {
    SourceChanged,
    ScopeChanged,
    Denied,
    Invalid,
    Conflict,
    Limit,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteWorkspaceProjectionError {
    pub code: RemoteWorkspaceErrorCode,
    pub message: &'static str,
}

pub trait RemoteWorkspaceProjection: Send + Sync + std::fmt::Debug {
    fn capabilities(
        &self,
        permission: RemoteViewPermission,
    ) -> std::result::Result<serde_json::Value, RemoteWorkspaceProjectionError>;
    fn view(
        &self,
        source_id: &str,
        scope_id: &str,
        request: &RemoteWorkspaceViewRequest,
    ) -> std::result::Result<serde_json::Value, RemoteWorkspaceProjectionError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteWorkspaceResponse {
    Capabilities { capabilities: serde_json::Value },
    View { envelope: serde_json::Value },
    Error {
        source_id: Option<String>,
        scope_id: Option<String>,
        code: RemoteWorkspaceErrorCode,
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineReadRequest {
    MachineInfo,
    Workspaces,
    FileList { path: String },
    FileRead { path: String },
    GitStatus,
    Sessions,
    ViewFileList { path: String },
    ViewFileRead { path: String },
    ViewGitStatus,
    RemoteWorkspace { workspace: RemoteWorkspaceRequest },
}

impl MachineReadRequest {
    pub fn required_capability(&self) -> MachineCapability {
        match self {
            Self::MachineInfo => MachineCapability::MachineInfo,
            Self::Workspaces => MachineCapability::WorkspaceDiscovery,
            Self::FileList { .. }
            | Self::FileRead { .. }
            | Self::ViewFileList { .. }
            | Self::ViewFileRead { .. } => MachineCapability::FileRead,
            Self::GitStatus | Self::ViewGitStatus => MachineCapability::GitRead,
            Self::RemoteWorkspace { workspace } => match workspace.permission() {
                RemoteViewPermission::Files => MachineCapability::FileRead,
                RemoteViewPermission::Git => MachineCapability::GitRead,
            },
            Self::Sessions => MachineCapability::SessionRead,
        }
    }

    fn remote_view_permission(&self) -> Option<RemoteViewPermission> {
        match self {
            Self::ViewFileList { .. } | Self::ViewFileRead { .. } => {
                Some(RemoteViewPermission::Files)
            }
            Self::ViewGitStatus => Some(RemoteViewPermission::Git),
            Self::RemoteWorkspace { workspace } => Some(workspace.permission()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteMachineInfo {
    pub node_id: String,
    pub display_name: String,
    pub status: String,
    pub security_epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteWorkspaceInfo {
    pub id: String,
    pub domain_id: String,
    pub name: String,
    pub kind: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteFileEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteSessionInfo {
    pub id: String,
    pub title: String,
    pub workspace_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineReadResponse {
    MachineInfo {
        machine: RemoteMachineInfo,
    },
    Workspaces {
        workspaces: Vec<RemoteWorkspaceInfo>,
    },
    FileList {
        path: String,
        entries: Vec<RemoteFileEntry>,
        incomplete: bool,
    },
    FileRead {
        path: String,
        text: String,
    },
    GitStatus {
        status: aw_git::Status,
    },
    Sessions {
        sessions: Vec<RemoteSessionInfo>,
    },
    RemoteWorkspace {
        workspace: RemoteWorkspaceResponse,
    },
}

#[derive(Clone, Debug)]
pub struct MachineServiceServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
    pub root: PathBuf,
    pub git: PathBuf,
    pub remote_view_workspace_id: Option<String>,
    pub remote_workspace: Option<Arc<dyn RemoteWorkspaceProjection>>,
}

#[derive(Clone, Debug)]
pub struct RemoteViewReadOptions {
    pub grant: RemoteViewGrant,
    pub signing_key: SigningKey,
}

#[derive(Clone, Debug)]
pub struct MachineServiceRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub request: MachineReadRequest,
    pub remote_view: Option<RemoteViewReadOptions>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MachineServiceResult {
    pub lease: MachineLinkLease,
    pub response: MachineReadResponse,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteViewReadHello {
    grant_id: String,
    domain_id: String,
    public_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteViewReadChallenge {
    version: u16,
    lease_id: String,
    request_id: String,
    grant_id: String,
    domain_id: String,
    workspace_id: String,
    public_key: String,
    target_node_id: String,
    target_store_id: String,
    peer_node_id: String,
    request_sha256: String,
    nonce: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteViewReadProof {
    version: u16,
    lease_id: String,
    request_id: String,
    grant_id: String,
    public_key: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceRequestEnvelope {
    version: u16,
    lease_id: String,
    request_id: String,
    request: MachineReadRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_view: Option<RemoteViewReadHello>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceResponseEnvelope {
    version: u16,
    lease_id: String,
    request_id: String,
    response: MachineReadResponse,
}

fn machine_service_version(request: &MachineReadRequest) -> u16 {
    match request {
        MachineReadRequest::RemoteWorkspace { .. } => REMOTE_WORKSPACE_SERVICE_VERSION,
        _ if request.remote_view_permission().is_some() => REMOTE_VIEW_SERVICE_VERSION,
        _ => MACHINE_SERVICE_VERSION,
    }
}

fn validate_service_mode(
    remote_view_workspace_id: Option<&str>,
    request: &MachineReadRequest,
) -> Result<Option<RemoteViewPermission>> {
    let permission = request.remote_view_permission();
    match (remote_view_workspace_id, permission) {
        (Some(_), None) => Err(Error::Denied(
            "remote-view Machine service rejects legacy ungranted reads",
        )),
        (None, Some(_)) => Err(Error::Denied(
            "ordinary Machine service does not accept remote-view reads",
        )),
        _ => Ok(permission),
    }
}

fn require_capability(
    capabilities: &[MachineCapability],
    required: MachineCapability,
    message: &'static str,
) -> Result<()> {
    if capabilities.contains(&required) {
        Ok(())
    } else {
        Err(Error::Denied(message))
    }
}

fn request_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for MachineLink request"))?;
    Ok(format!("mr_{}", hex(&bytes)))
}

fn remote_view_nonce() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for remote-view challenge"))?;
    Ok(hex(&bytes))
}

fn request_sha256(request: &MachineReadRequest) -> Result<String> {
    let encoded = serde_json::to_vec(request)
        .map_err(|_| Error::Storage("cannot encode remote-view read request"))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn remote_view_read_transcript(
    challenge: &RemoteViewReadChallenge,
    public_key: &str,
) -> Vec<u8> {
    let fields: [&[u8]; 12] = [
        challenge.lease_id.as_bytes(),
        challenge.request_id.as_bytes(),
        challenge.grant_id.as_bytes(),
        challenge.domain_id.as_bytes(),
        challenge.workspace_id.as_bytes(),
        challenge.target_node_id.as_bytes(),
        challenge.target_store_id.as_bytes(),
        challenge.peer_node_id.as_bytes(),
        challenge.request_sha256.as_bytes(),
        challenge.nonce.as_bytes(),
        challenge.public_key.as_bytes(),
        public_key.as_bytes(),
    ];
    let mut out = Vec::with_capacity(768);
    out.extend_from_slice(b"aw-remote-view-read-v1\0");
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

fn authenticate_remote_view_read(
    store: &SessionStore,
    signed_peer_node_id: &str,
    server_workspace_id: &str,
    permission: RemoteViewPermission,
    challenge: &RemoteViewReadChallenge,
    proof: &RemoteViewReadProof,
) -> Result<AuthenticatedStableUser> {
    if !matches!(
        challenge.version,
        REMOTE_VIEW_SERVICE_VERSION | REMOTE_WORKSPACE_SERVICE_VERSION
    ) || proof.version != challenge.version
        || proof.lease_id != challenge.lease_id
        || proof.request_id != challenge.request_id
        || proof.grant_id != challenge.grant_id
        || proof.public_key != challenge.public_key
    {
        return Err(Error::Protocol(
            "remote-view read proof correlation failed",
        ));
    }
    let authenticated = store.authenticate_portable_device_signature(
        &challenge.domain_id,
        &proof.public_key,
        &remote_view_read_transcript(challenge, &proof.public_key),
        &proof.signature,
    )?;
    let grant = store.authorize_remote_view(
        &challenge.grant_id,
        &authenticated,
        signed_peer_node_id,
        permission,
    )?;
    if grant.workspace_id != server_workspace_id
        || grant.domain_id != challenge.domain_id
        || grant.workspace_id != challenge.workspace_id
    {
        return Err(Error::Denied(
            "remote-view grant does not match the exported Workspace",
        ));
    }
    Ok(authenticated)
}

fn remote_workspace_response(
    projection: Option<&dyn RemoteWorkspaceProjection>,
    request: &RemoteWorkspaceRequest,
) -> Result<MachineReadResponse> {
    let projection = projection.ok_or(Error::Denied(
        "remote Workspace projection was not enabled by the serving host",
    ))?;
    let workspace = match request {
        RemoteWorkspaceRequest::Capabilities { permission } => {
            match projection.capabilities(*permission) {
                Ok(capabilities) => RemoteWorkspaceResponse::Capabilities { capabilities },
                Err(error) => RemoteWorkspaceResponse::Error {
                    source_id: None,
                    scope_id: None,
                    code: error.code,
                    message: error.message.into(),
                },
            }
        }
        RemoteWorkspaceRequest::View {
            source_id,
            scope_id,
            request,
        } => match projection.view(source_id, scope_id, request) {
            Ok(envelope) => RemoteWorkspaceResponse::View { envelope },
            Err(error) => RemoteWorkspaceResponse::Error {
                source_id: Some(source_id.clone()),
                scope_id: Some(scope_id.clone()),
                code: error.code,
                message: error.message.into(),
            },
        },
    };
    Ok(MachineReadResponse::RemoteWorkspace { workspace })
}

fn execute_read_request(
    store: &SessionStore,
    actor: &PrincipalId,
    root: &Path,
    git: &Path,
    lease: &MachineLinkLease,
    request: &MachineReadRequest,
    remote_workspace: Option<&dyn RemoteWorkspaceProjection>,
) -> Result<MachineReadResponse> {
    if lease.expires_at <= now_secs() {
        return Err(Error::Denied("MachineLink lease expired"));
    }
    let needed = request.required_capability();
    if !lease.capabilities.contains(&needed) {
        return Err(Error::Denied(
            "MachineLink lease does not grant this typed service",
        ));
    }
    match request {
        MachineReadRequest::MachineInfo => {
            let machine = store.local_machine()?;
            Ok(MachineReadResponse::MachineInfo {
                machine: RemoteMachineInfo {
                    node_id: machine.node_id,
                    display_name: machine.display_name,
                    status: machine.status,
                    security_epoch: machine.security_epoch,
                },
            })
        }
        MachineReadRequest::Workspaces => {
            let workspaces = store
                .workspaces(actor, None)?
                .into_iter()
                .map(|workspace| RemoteWorkspaceInfo {
                    id: workspace.id,
                    domain_id: workspace.domain_id,
                    name: workspace.name,
                    kind: match workspace.kind {
                        aw_store::WorkspaceKind::Hosted => "hosted".into(),
                        aw_store::WorkspaceKind::FleetVirtual => "fleet_virtual".into(),
                    },
                    revision: workspace.revision,
                })
                .collect();
            Ok(MachineReadResponse::Workspaces { workspaces })
        }
        MachineReadRequest::FileList { path } | MachineReadRequest::ViewFileList { path } => {
            let listing = WorkspaceFiles::open(root)
                .map_err(|_| Error::Denied("exported file root is unavailable"))?
                .list(path)
                .map_err(|_| Error::Denied("file path is unavailable within the exported root"))?;
            let entries = listing
                .entries
                .into_iter()
                .map(|entry| RemoteFileEntry {
                    name: entry.name,
                    path: entry.path,
                    kind: match entry.kind {
                        EntryKind::Directory => "directory".into(),
                        EntryKind::File => "file".into(),
                        EntryKind::Unsupported => "unsupported".into(),
                    },
                })
                .collect();
            Ok(MachineReadResponse::FileList {
                path: listing.path,
                entries,
                incomplete: listing.incomplete,
            })
        }
        MachineReadRequest::FileRead { path } | MachineReadRequest::ViewFileRead { path } => {
            let text = WorkspaceFiles::open(root)
                .map_err(|_| Error::Denied("exported file root is unavailable"))?
                .read(path)
                .map_err(|_| Error::Denied("file path is unavailable within the exported root"))?;
            Ok(MachineReadResponse::FileRead {
                path: path.clone(),
                text,
            })
        }
        MachineReadRequest::GitStatus | MachineReadRequest::ViewGitStatus => {
            let repository = aw_git::Repository::discover(git, root)
                .map_err(|_| Error::Denied("exported root is not an available Git repository"))?;
            let status = repository
                .status()
                .map_err(|_| Error::Denied("Git status is unavailable for the exported root"))?;
            Ok(MachineReadResponse::GitStatus { status })
        }
        MachineReadRequest::RemoteWorkspace { workspace } => {
            remote_workspace_response(remote_workspace, workspace)
        }
        MachineReadRequest::Sessions => {
            let sessions = store
                .list(actor)?
                .into_iter()
                .map(|session| RemoteSessionInfo {
                    id: session.id,
                    title: session.title,
                    workspace_id: session.workspace_id,
                    created_at: session.created_at,
                    updated_at: session.updated_at,
                })
                .collect();
            Ok(MachineReadResponse::Sessions { sessions })
        }
    }
}

pub fn serve_machine_service(
    store: &SessionStore,
    state_dir: &Path,
    opts: MachineServiceServeOptions,
) -> Result<MachineServiceResult> {
    validate_caps(&opts.link.allowed)?;
    validate_lease(opts.link.lease)?;
    let root = opts
        .root
        .canonicalize()
        .map_err(|_| Error::Config("Machine service root must be an existing directory".into()))?;
    if !root.is_dir() || !opts.git.is_absolute() {
        return Err(Error::Config(
            "Machine service requires an existing root and absolute trusted Git executable".into(),
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let target_store_id = store.store_id()?;
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
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("Machine service request stream was not opened"))?;
        let envelope: ServiceRequestEnvelope = read_frame(&mut recv).await?;
        let protocol_version = machine_service_version(&envelope.request);
        if envelope.version != protocol_version || envelope.lease_id != lease.lease_id {
            return Err(Error::Protocol("Machine service lease/request mismatch"));
        }
        let remote_permission =
            validate_service_mode(opts.remote_view_workspace_id.as_deref(), &envelope.request)?;
        let remote_authorization = if let Some(permission) = remote_permission {
            require_capability(
                &lease.capabilities,
                MachineCapability::IdentityAuthenticate,
                "MachineLink lease does not grant remote-view device authentication",
            )?;
            let hello = envelope.remote_view.as_ref().ok_or(Error::Denied(
                "remote-view read requires an exact grant and device proof",
            ))?;
            let workspace_id = opts.remote_view_workspace_id.as_deref().ok_or(Error::Denied(
                "Machine service did not bind this exported root to a remote-view Workspace",
            ))?;
            let challenge = RemoteViewReadChallenge {
                version: protocol_version,
                lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id.clone(),
                grant_id: hello.grant_id.clone(),
                domain_id: hello.domain_id.clone(),
                workspace_id: workspace_id.to_owned(),
                public_key: hello.public_key.clone(),
                target_node_id: identity.node_id.clone(),
                target_store_id: target_store_id.clone(),
                peer_node_id: peer.node_id.clone(),
                request_sha256: request_sha256(&envelope.request)?,
                nonce: remote_view_nonce()?,
            };
            write_frame(&mut send, &challenge).await?;
            let proof: RemoteViewReadProof = read_frame(&mut recv).await?;
            let authenticated = authenticate_remote_view_read(
                store,
                &peer.node_id,
                workspace_id,
                permission,
                &challenge,
                &proof,
            )?;
            Some((
                authenticated,
                permission,
                hello.grant_id.clone(),
                workspace_id.to_owned(),
            ))
        } else {
            if envelope.remote_view.is_some() {
                return Err(Error::Protocol(
                    "remote-view authorization was attached to a legacy Machine read",
                ));
            }
            None
        };
        let response = execute_read_request(
            store,
            &opts.actor,
            &root,
            &opts.git,
            &lease,
            &envelope.request,
            opts.remote_workspace.as_deref(),
        )?;
        if let Some((authenticated, permission, grant_id, workspace_id)) = &remote_authorization {
            let grant = store.authorize_remote_view(
                grant_id,
                authenticated,
                &peer.node_id,
                *permission,
            )?;
            if grant.workspace_id != *workspace_id {
                return Err(Error::Denied(
                    "remote-view grant changed before response publication",
                ));
            }
        }
        write_frame(
            &mut send,
            &ServiceResponseEnvelope {
                version: protocol_version,
                lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id,
                response: response.clone(),
            },
        )
        .await?;
        Ok::<_, Error>(MachineServiceResult { lease, response })
    });
    link.connection
        .close(0u32.into(), b"Machine service complete");
    link.endpoint
        .close(0u32.into(), b"Machine service complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_machine_service(
    store: &SessionStore,
    state_dir: &Path,
    opts: MachineServiceRequestOptions,
) -> Result<MachineServiceResult> {
    validate_caps(&opts.link.requested)?;
    validate_lease(opts.link.lease)?;
    let required = opts.request.required_capability();
    if !opts.link.requested.contains(&required) {
        return Err(Error::Config(
            "Machine service request capability must be present in the requested lease".into(),
        ));
    }
    let remote_permission = opts.request.remote_view_permission();
    match (remote_permission, opts.remote_view.as_ref()) {
        (Some(permission), Some(remote)) => {
            if !opts
                .link
                .requested
                .contains(&MachineCapability::IdentityAuthenticate)
                || !remote.grant.permissions.contains(&permission)
            {
                return Err(Error::Config(
                    "remote-view read requires identity-authenticate and the exact grant permission"
                        .into(),
                ));
            }
            let public_key = hex(&remote.signing_key.verifying_key().to_bytes());
            if public_key != remote.grant.device_public_key {
                return Err(Error::Denied(
                    "remote-view signing key does not match the granted device",
                ));
            }
        }
        (Some(_), None) => {
            return Err(Error::Config(
                "remote-view read requires a persisted grant and signing key".into(),
            ))
        }
        (None, Some(_)) => {
            return Err(Error::Config(
                "remote-view authorization cannot be attached to a legacy Machine read".into(),
            ))
        }
        (None, None) => {}
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    if let Some(remote) = &opts.remote_view {
        if remote.grant.requester_machine_node_id != identity.node_id
            || remote.grant.target_machine_node_id != peer.node_id
        {
            return Err(Error::Denied(
                "remote-view grant is bound to different requester/target Machines",
            ));
        }
    }
    let link = open_punched_quic(store, state_dir, opts.link.route)?;
    let result = link.runtime.block_on(async {
        let lease = negotiate_client(
            &link.connection,
            &identity,
            &peer,
            &opts.link.requested,
            opts.link.lease,
        )
        .await?;
        if !lease.capabilities.contains(&required) {
            return Err(Error::Denied(
                "Machine service was not granted by the serving peer",
            ));
        }
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open Machine service request stream"))?;
        let request_id = request_id()?;
        let protocol_version = machine_service_version(&opts.request);
        let request_hash = request_sha256(&opts.request)?;
        let remote_hello = opts.remote_view.as_ref().map(|remote| RemoteViewReadHello {
            grant_id: remote.grant.grant_id.clone(),
            domain_id: remote.grant.domain_id.clone(),
            public_key: hex(&remote.signing_key.verifying_key().to_bytes()),
        });
        write_frame(
            &mut send,
            &ServiceRequestEnvelope {
                version: protocol_version,
                lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request: opts.request.clone(),
                remote_view: remote_hello,
            },
        )
        .await?;
        if let Some(remote) = &opts.remote_view {
            let challenge: RemoteViewReadChallenge = read_frame(&mut recv).await?;
            if challenge.version != protocol_version
                || challenge.lease_id != lease.lease_id
                || challenge.request_id != request_id
                || challenge.grant_id != remote.grant.grant_id
                || challenge.domain_id != remote.grant.domain_id
                || challenge.workspace_id != remote.grant.workspace_id
                || challenge.public_key != remote.grant.device_public_key
                || challenge.target_node_id != peer.node_id
                || challenge.peer_node_id != identity.node_id
                || challenge.request_sha256 != request_hash
            {
                return Err(Error::Protocol(
                    "remote-view read challenge correlation failed",
                ));
            }
            let signature = STANDARD.encode(
                remote
                    .signing_key
                    .sign(&remote_view_read_transcript(
                        &challenge,
                        &remote.grant.device_public_key,
                    ))
                    .to_bytes(),
            );
            write_frame(
                &mut send,
                &RemoteViewReadProof {
                    version: protocol_version,
                    lease_id: lease.lease_id.clone(),
                    request_id: request_id.clone(),
                    grant_id: remote.grant.grant_id.clone(),
                    public_key: remote.grant.device_public_key.clone(),
                    signature,
                },
            )
            .await?;
        }
        let response: ServiceResponseEnvelope = read_frame(&mut recv).await?;
        if response.version != protocol_version
            || response.lease_id != lease.lease_id
            || response.request_id != request_id
        {
            return Err(Error::Protocol(
                "Machine service response correlation failed",
            ));
        }
        Ok::<_, Error>(MachineServiceResult {
            lease,
            response: response.response,
        })
    });
    link.connection
        .close(0u32.into(), b"Machine service complete");
    link.endpoint
        .close(0u32.into(), b"Machine service complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_read_requests_map_to_exact_capabilities() {
        assert_eq!(
            MachineReadRequest::MachineInfo.required_capability(),
            MachineCapability::MachineInfo
        );
        assert_eq!(
            MachineReadRequest::FileRead {
                path: "a.txt".into()
            }
            .required_capability(),
            MachineCapability::FileRead
        );
        assert_eq!(
            MachineReadRequest::Sessions.required_capability(),
            MachineCapability::SessionRead
        );
        assert_eq!(
            MachineReadRequest::ViewFileRead {
                path: "a.txt".into()
            }
            .required_capability(),
            MachineCapability::FileRead
        );
        assert_eq!(
            MachineReadRequest::ViewFileRead {
                path: "a.txt".into()
            }
            .remote_view_permission(),
            Some(RemoteViewPermission::Files)
        );
        assert_eq!(
            MachineReadRequest::ViewGitStatus.remote_view_permission(),
            Some(RemoteViewPermission::Git)
        );
        assert_eq!(
            MachineReadRequest::FileRead {
                path: "a.txt".into()
            }
            .remote_view_permission(),
            None
        );
        let remote_files = MachineReadRequest::RemoteWorkspace {
            workspace: RemoteWorkspaceRequest::View {
                source_id: "surface_fixture".into(),
                scope_id: "scope_fixture".into(),
                request: RemoteWorkspaceViewRequest::File {
                    path: "a.txt".into(),
                },
            },
        };
        assert_eq!(
            remote_files.required_capability(),
            MachineCapability::FileRead
        );
        assert_eq!(
            remote_files.remote_view_permission(),
            Some(RemoteViewPermission::Files)
        );
        assert_eq!(
            machine_service_version(&remote_files),
            REMOTE_WORKSPACE_SERVICE_VERSION
        );
        let remote_git = MachineReadRequest::RemoteWorkspace {
            workspace: RemoteWorkspaceRequest::View {
                source_id: "surface_fixture".into(),
                scope_id: "scope_fixture".into(),
                request: RemoteWorkspaceViewRequest::GitList {
                    mode: RemoteWorkspaceGitMode::Changes,
                },
            },
        };
        assert_eq!(remote_git.required_capability(), MachineCapability::GitRead);
        assert_eq!(
            remote_git.remote_view_permission(),
            Some(RemoteViewPermission::Git)
        );
    }

    #[test]
    fn remote_view_service_mode_fails_closed_against_protocol_downgrade() {
        let legacy = MachineReadRequest::FileRead {
            path: "src/lib.rs".into(),
        };
        let protected = MachineReadRequest::ViewFileRead {
            path: "src/lib.rs".into(),
        };
        assert!(validate_service_mode(Some("ws_fixture"), &legacy).is_err());
        assert!(validate_service_mode(None, &protected).is_err());
        assert_eq!(
            validate_service_mode(Some("ws_fixture"), &protected).unwrap(),
            Some(RemoteViewPermission::Files)
        );
        assert_eq!(validate_service_mode(None, &legacy).unwrap(), None);
    }

    #[test]
    fn legacy_machine_read_v1_omits_remote_view_wire_field() {
        let request = MachineReadRequest::FileRead {
            path: "src/lib.rs".into(),
        };
        assert_eq!(machine_service_version(&request), MACHINE_SERVICE_VERSION);
        assert_eq!(
            machine_service_version(&MachineReadRequest::ViewFileRead {
                path: "src/lib.rs".into(),
            }),
            REMOTE_VIEW_SERVICE_VERSION
        );

        let envelope = ServiceRequestEnvelope {
            version: MACHINE_SERVICE_VERSION,
            lease_id: "ml_fixture".into(),
            request_id: "mr_fixture".into(),
            request,
            remote_view: None,
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert!(json.get("remote_view").is_none());
    }

    #[derive(Debug)]
    struct Projection;

    impl RemoteWorkspaceProjection for Projection {
        fn capabilities(
            &self,
            permission: RemoteViewPermission,
        ) -> std::result::Result<serde_json::Value, RemoteWorkspaceProjectionError> {
            Ok(serde_json::json!({
                "source_id":"surface_fixture",
                "view_root":{"id":"scope_fixture","label":"remote"},
                "files": permission == RemoteViewPermission::Files,
                "git_read": permission == RemoteViewPermission::Git
            }))
        }

        fn view(
            &self,
            source_id: &str,
            scope_id: &str,
            request: &RemoteWorkspaceViewRequest,
        ) -> std::result::Result<serde_json::Value, RemoteWorkspaceProjectionError> {
            if source_id != "surface_fixture" {
                return Err(RemoteWorkspaceProjectionError {
                    code: RemoteWorkspaceErrorCode::SourceChanged,
                    message: "source changed",
                });
            }
            Ok(serde_json::json!({
                "source_id": source_id,
                "scope_id": scope_id,
                "result": {"request": request}
            }))
        }
    }

    #[test]
    fn remote_workspace_projection_preserves_typed_errors_as_read_results() {
        let projection = Projection;
        let capabilities = remote_workspace_response(
            Some(&projection),
            &RemoteWorkspaceRequest::Capabilities {
                permission: RemoteViewPermission::Files,
            },
        )
        .unwrap();
        match capabilities {
            MachineReadResponse::RemoteWorkspace {
                workspace: RemoteWorkspaceResponse::Capabilities { capabilities },
            } => {
                assert_eq!(capabilities["files"], true);
                assert_eq!(capabilities["git_read"], false);
            }
            other => panic!("unexpected capabilities response: {other:?}"),
        }

        let denied = remote_workspace_response(
            Some(&projection),
            &RemoteWorkspaceRequest::View {
                source_id: "surface_wrong".into(),
                scope_id: "scope_fixture".into(),
                request: RemoteWorkspaceViewRequest::File {
                    path: "src/lib.rs".into(),
                },
            },
        )
        .unwrap();
        match denied {
            MachineReadResponse::RemoteWorkspace {
                workspace:
                    RemoteWorkspaceResponse::Error {
                        source_id,
                        scope_id,
                        code,
                        ..
                    },
            } => {
                assert_eq!(source_id.as_deref(), Some("surface_wrong"));
                assert_eq!(scope_id.as_deref(), Some("scope_fixture"));
                assert_eq!(code, RemoteWorkspaceErrorCode::SourceChanged);
            }
            other => panic!("unexpected error response: {other:?}"),
        }
    }

    #[test]
    fn remote_workspace_v3_request_hash_binds_full_git_operation() {
        let a = MachineReadRequest::RemoteWorkspace {
            workspace: RemoteWorkspaceRequest::View {
                source_id: "surface_fixture".into(),
                scope_id: "scope_fixture".into(),
                request: RemoteWorkspaceViewRequest::GitOpen {
                    inventory_id: "inventory_fixture".into(),
                    item_id: "item_a".into(),
                },
            },
        };
        let b = MachineReadRequest::RemoteWorkspace {
            workspace: RemoteWorkspaceRequest::View {
                source_id: "surface_fixture".into(),
                scope_id: "scope_fixture".into(),
                request: RemoteWorkspaceViewRequest::GitOpen {
                    inventory_id: "inventory_fixture".into(),
                    item_id: "item_b".into(),
                },
            },
        };
        assert_eq!(machine_service_version(&a), REMOTE_WORKSPACE_SERVICE_VERSION);
        assert_ne!(request_sha256(&a).unwrap(), request_sha256(&b).unwrap());
    }

    #[test]
    fn remote_view_read_transcript_binds_grant_workspace_peer_and_request() {
        let request = MachineReadRequest::ViewFileRead {
            path: "src/lib.rs".into(),
        };
        let mut challenge = RemoteViewReadChallenge {
            version: REMOTE_VIEW_SERVICE_VERSION,
            lease_id: "ml_fixture".into(),
            request_id: "mr_fixture".into(),
            grant_id: "rvg_fixture".into(),
            domain_id: "dom_fixture".into(),
            workspace_id: "ws_fixture".into(),
            public_key: "11".repeat(32),
            target_node_id: "node_target".into(),
            target_store_id: "store_target".into(),
            peer_node_id: "node_client".into(),
            request_sha256: request_sha256(&request).unwrap(),
            nonce: "aa".repeat(32),
        };
        let key = challenge.public_key.clone();
        let baseline = remote_view_read_transcript(&challenge, &key);

        challenge.grant_id = "rvg_other".into();
        assert_ne!(baseline, remote_view_read_transcript(&challenge, &key));
        challenge.grant_id = "rvg_fixture".into();

        challenge.workspace_id = "ws_other".into();
        assert_ne!(baseline, remote_view_read_transcript(&challenge, &key));
        challenge.workspace_id = "ws_fixture".into();

        challenge.request_sha256 = request_sha256(&MachineReadRequest::ViewFileRead {
            path: "src/main.rs".into(),
        })
        .unwrap();
        assert_ne!(baseline, remote_view_read_transcript(&challenge, &key));
    }
}
