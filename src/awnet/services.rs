use super::machinelink::{
    negotiate_client, negotiate_server, validate_caps, validate_lease, MachineCapability,
    MachineLinkConnectOptions, MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_tools::browse::{EntryKind, WorkspaceFiles};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

const MACHINE_SERVICE_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum MachineReadRequest {
    MachineInfo,
    Workspaces,
    FileList { path: String },
    FileRead { path: String },
    GitStatus,
    Sessions,
}

impl MachineReadRequest {
    pub fn required_capability(&self) -> MachineCapability {
        match self {
            Self::MachineInfo => MachineCapability::MachineInfo,
            Self::Workspaces => MachineCapability::WorkspaceDiscovery,
            Self::FileList { .. } | Self::FileRead { .. } => MachineCapability::FileRead,
            Self::GitStatus => MachineCapability::GitRead,
            Self::Sessions => MachineCapability::SessionRead,
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
}

#[derive(Clone, Debug)]
pub struct MachineServiceServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
    pub root: PathBuf,
    pub git: PathBuf,
}

#[derive(Clone, Debug)]
pub struct MachineServiceRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub request: MachineReadRequest,
}

#[derive(Clone, Debug, Serialize)]
pub struct MachineServiceResult {
    pub lease: MachineLinkLease,
    pub response: MachineReadResponse,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceRequestEnvelope {
    version: u16,
    lease_id: String,
    request_id: String,
    request: MachineReadRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceResponseEnvelope {
    version: u16,
    lease_id: String,
    request_id: String,
    response: MachineReadResponse,
}

fn request_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for MachineLink request"))?;
    Ok(format!("mr_{}", hex(&bytes)))
}

fn execute_read_request(
    store: &SessionStore,
    actor: &PrincipalId,
    root: &Path,
    git: &Path,
    lease: &MachineLinkLease,
    request: &MachineReadRequest,
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
        MachineReadRequest::FileList { path } => {
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
        MachineReadRequest::FileRead { path } => {
            let text = WorkspaceFiles::open(root)
                .map_err(|_| Error::Denied("exported file root is unavailable"))?
                .read(path)
                .map_err(|_| Error::Denied("file path is unavailable within the exported root"))?;
            Ok(MachineReadResponse::FileRead {
                path: path.clone(),
                text,
            })
        }
        MachineReadRequest::GitStatus => {
            let repository = aw_git::Repository::discover(git, root)
                .map_err(|_| Error::Denied("exported root is not an available Git repository"))?;
            let status = repository
                .status()
                .map_err(|_| Error::Denied("Git status is unavailable for the exported root"))?;
            Ok(MachineReadResponse::GitStatus { status })
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
        if envelope.version != MACHINE_SERVICE_VERSION || envelope.lease_id != lease.lease_id {
            return Err(Error::Protocol("Machine service lease/request mismatch"));
        }
        let response = execute_read_request(
            store,
            &opts.actor,
            &root,
            &opts.git,
            &lease,
            &envelope.request,
        )?;
        write_frame(
            &mut send,
            &ServiceResponseEnvelope {
                version: MACHINE_SERVICE_VERSION,
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
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
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
        write_frame(
            &mut send,
            &ServiceRequestEnvelope {
                version: MACHINE_SERVICE_VERSION,
                lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request: opts.request,
            },
        )
        .await?;
        let response: ServiceResponseEnvelope = read_frame(&mut recv).await?;
        if response.version != MACHINE_SERVICE_VERSION
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
    }
}
