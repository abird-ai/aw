use super::machinelink::{
    negotiate_client, negotiate_server, MachineCapability, MachineLinkConnectOptions,
    MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_store::{
    workspace_replica_snapshot_transcript, BranchInfo, WorkspaceReplicaApplyResult,
    WorkspaceReplicaSnapshot, WorkspaceWriterEventInfo, WorkspaceWriterInfo,
};
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::Signer;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

const WORKSPACE_REPLICA_STREAM_VERSION: u16 = 1;
const WORKSPACE_WRITER_STREAM_VERSION: u16 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceReplicaRequest {
    workspace_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceReplicaEnvelope {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    request: WorkspaceReplicaRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceReplicaReply {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    snapshot: WorkspaceReplicaSnapshot,
    source_signature: String,
}

#[derive(Clone, Debug)]
pub struct WorkspaceReplicaServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
    pub workspace_id: String,
}

#[derive(Clone, Debug)]
pub struct WorkspaceReplicaRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub actor: PrincipalId,
    pub workspace_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceReplicaServeResult {
    pub lease: MachineLinkLease,
    pub workspace_id: String,
    pub snapshot_sha256: String,
    pub writer_epoch: u64,
    pub authority_term: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceReplicaRequestResult {
    pub lease: MachineLinkLease,
    pub applied: WorkspaceReplicaApplyResult,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceWriterRequest {
    Fence {
        workspace_id: String,
        event: WorkspaceWriterEventInfo,
    },
    Branches {
        actor_user_id: String,
        workspace_id: String,
        writer_epoch: u64,
        authority_term: u64,
        session_id: String,
    },
    Checkpoint {
        actor_user_id: String,
        workspace_id: String,
        writer_epoch: u64,
        authority_term: u64,
        session_id: String,
        branch_id: String,
        expected_head: String,
        expected_revision: u64,
        summary: String,
    },
    Fork {
        actor_user_id: String,
        workspace_id: String,
        writer_epoch: u64,
        authority_term: u64,
        session_id: String,
        name: String,
        at: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceWriterResponse {
    Writer { writer: WorkspaceWriterInfo },
    Branches { branches: Vec<BranchInfo> },
    Branch { branch: BranchInfo },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceWriterEnvelope {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    request: WorkspaceWriterRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceWriterReply {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    response: WorkspaceWriterResponse,
}

#[derive(Clone, Debug)]
pub struct WorkspaceWriterServeOptions {
    pub link: MachineLinkServeOptions,
}

#[derive(Clone, Debug)]
pub struct WorkspaceWriterRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub request: WorkspaceWriterRequest,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceWriterServeResult {
    pub lease: MachineLinkLease,
    pub response: WorkspaceWriterResponse,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceWriterRequestResult {
    pub lease: MachineLinkLease,
    pub response: WorkspaceWriterResponse,
}

fn request_id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for Workspace request"))?;
    Ok(format!("{prefix}_{}", hex(&bytes)))
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

pub fn serve_workspace_replica_snapshot(
    store: &SessionStore,
    state_dir: &Path,
    opts: WorkspaceReplicaServeOptions,
) -> Result<WorkspaceReplicaServeResult> {
    if !opts
        .link
        .allowed
        .contains(&MachineCapability::WorkspaceReplicaHandoff)
    {
        return Err(Error::Config(
            "Workspace replica server must explicitly grant workspace-replica-handoff".into(),
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let snapshot =
        store.export_workspace_replica_snapshot(&opts.actor, &opts.workspace_id, &peer.node_id)?;
    if snapshot.source_node_id != identity.node_id {
        return Err(Error::Conflict(
            "Workspace replica snapshot source does not match this AwNode identity",
        ));
    }
    let source_signature = STANDARD.encode(
        identity
            .signing
            .sign(&workspace_replica_snapshot_transcript(&snapshot))
            .to_bytes(),
    );
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
        require_capability(
            &lease.capabilities,
            MachineCapability::WorkspaceReplicaHandoff,
            "MachineLink lease does not grant Workspace replica handoff",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("Workspace replica stream was not opened"))?;
        let envelope: WorkspaceReplicaEnvelope = read_data_frame(&mut recv).await?;
        if envelope.version != WORKSPACE_REPLICA_STREAM_VERSION
            || envelope.machine_lease_id != lease.lease_id
            || envelope.request.workspace_id != snapshot.workspace.workspace_id
        {
            return Err(Error::Protocol(
                "Workspace replica request is not bound to the negotiated lease/workspace",
            ));
        }
        write_data_frame(
            &mut send,
            &WorkspaceReplicaReply {
                version: WORKSPACE_REPLICA_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id,
                snapshot: snapshot.clone(),
                source_signature: source_signature.clone(),
            },
        )
        .await?;
        Ok::<_, Error>(WorkspaceReplicaServeResult {
            lease,
            workspace_id: snapshot.workspace.workspace_id.clone(),
            snapshot_sha256: snapshot.snapshot_sha256.clone(),
            writer_epoch: snapshot.writer.writer_epoch,
            authority_term: snapshot.writer.authority_term,
        })
    });
    link.connection
        .close(0u32.into(), b"Workspace replica handoff complete");
    link.endpoint
        .close(0u32.into(), b"Workspace replica handoff complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_workspace_replica_snapshot(
    store: &SessionStore,
    state_dir: &Path,
    opts: WorkspaceReplicaRequestOptions,
) -> Result<WorkspaceReplicaRequestResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::WorkspaceReplicaHandoff)
    {
        return Err(Error::Config(
            "Workspace replica client must explicitly request workspace-replica-handoff".into(),
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
        require_capability(
            &lease.capabilities,
            MachineCapability::WorkspaceReplicaHandoff,
            "MachineLink lease does not grant Workspace replica handoff",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open Workspace replica stream"))?;
        let request_id = request_id("wrq")?;
        write_data_frame(
            &mut send,
            &WorkspaceReplicaEnvelope {
                version: WORKSPACE_REPLICA_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request: WorkspaceReplicaRequest {
                    workspace_id: opts.workspace_id.clone(),
                },
            },
        )
        .await?;
        let reply: WorkspaceReplicaReply = read_data_frame(&mut recv).await?;
        if reply.version != WORKSPACE_REPLICA_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
            || reply.snapshot.workspace.workspace_id != opts.workspace_id
            || reply.snapshot.source_node_id != peer.node_id
        {
            return Err(Error::Protocol(
                "Workspace replica response correlation failed",
            ));
        }
        verify_signed_identity(
            &peer.node_id,
            &peer.public_key,
            &workspace_replica_snapshot_transcript(&reply.snapshot),
            &reply.source_signature,
        )?;
        let applied = store.import_workspace_replica_snapshot(
            &opts.actor,
            &peer.node_id,
            &reply.snapshot,
            &reply.source_signature,
        )?;
        Ok::<_, Error>(WorkspaceReplicaRequestResult { lease, applied })
    });
    link.connection
        .close(0u32.into(), b"Workspace replica handoff complete");
    link.endpoint
        .close(0u32.into(), b"Workspace replica handoff complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

fn execute_writer_request(
    store: &SessionStore,
    peer_node_id: &str,
    request: WorkspaceWriterRequest,
) -> Result<WorkspaceWriterResponse> {
    match request {
        WorkspaceWriterRequest::Fence {
            workspace_id,
            event,
        } => Ok(WorkspaceWriterResponse::Writer {
            writer: store.replica_writer_fence(peer_node_id, &workspace_id, &event)?,
        }),
        WorkspaceWriterRequest::Branches {
            actor_user_id,
            workspace_id,
            writer_epoch,
            authority_term,
            session_id,
        } => Ok(WorkspaceWriterResponse::Branches {
            branches: store.replica_writer_branches(
                peer_node_id,
                &aw_store::stable_user_principal(&actor_user_id)?,
                &workspace_id,
                writer_epoch,
                authority_term,
                &session_id,
            )?,
        }),
        WorkspaceWriterRequest::Checkpoint {
            actor_user_id,
            workspace_id,
            writer_epoch,
            authority_term,
            session_id,
            branch_id,
            expected_head,
            expected_revision,
            summary,
        } => Ok(WorkspaceWriterResponse::Branch {
            branch: store.replica_writer_checkpoint(
                peer_node_id,
                &aw_store::stable_user_principal(&actor_user_id)?,
                &workspace_id,
                writer_epoch,
                authority_term,
                &session_id,
                &branch_id,
                &expected_head,
                expected_revision,
                &summary,
            )?,
        }),
        WorkspaceWriterRequest::Fork {
            actor_user_id,
            workspace_id,
            writer_epoch,
            authority_term,
            session_id,
            name,
            at,
        } => Ok(WorkspaceWriterResponse::Branch {
            branch: store.replica_writer_fork(
                peer_node_id,
                &aw_store::stable_user_principal(&actor_user_id)?,
                &workspace_id,
                writer_epoch,
                authority_term,
                &session_id,
                &name,
                &at,
            )?,
        }),
    }
}

pub fn serve_workspace_writer_request(
    store: &SessionStore,
    state_dir: &Path,
    opts: WorkspaceWriterServeOptions,
) -> Result<WorkspaceWriterServeResult> {
    if !opts
        .link
        .allowed
        .contains(&MachineCapability::WorkspaceWriterRef)
    {
        return Err(Error::Config(
            "Workspace writer server must explicitly grant workspace-writer-ref".into(),
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
        require_capability(
            &lease.capabilities,
            MachineCapability::WorkspaceWriterRef,
            "MachineLink lease does not grant Workspace writer ref control",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("Workspace writer ref stream was not opened"))?;
        let envelope: WorkspaceWriterEnvelope = read_data_frame(&mut recv).await?;
        if envelope.version != WORKSPACE_WRITER_STREAM_VERSION
            || envelope.machine_lease_id != lease.lease_id
        {
            return Err(Error::Protocol(
                "Workspace writer request is not bound to the negotiated MachineLink lease",
            ));
        }
        let response = execute_writer_request(store, &peer.node_id, envelope.request)?;
        write_data_frame(
            &mut send,
            &WorkspaceWriterReply {
                version: WORKSPACE_WRITER_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id,
                response: response.clone(),
            },
        )
        .await?;
        Ok::<_, Error>(WorkspaceWriterServeResult { lease, response })
    });
    link.connection
        .close(0u32.into(), b"Workspace writer ref operation complete");
    link.endpoint
        .close(0u32.into(), b"Workspace writer ref operation complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_workspace_writer_service(
    store: &SessionStore,
    state_dir: &Path,
    opts: WorkspaceWriterRequestOptions,
) -> Result<WorkspaceWriterRequestResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::WorkspaceWriterRef)
    {
        return Err(Error::Config(
            "Workspace writer client must explicitly request workspace-writer-ref".into(),
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
        require_capability(
            &lease.capabilities,
            MachineCapability::WorkspaceWriterRef,
            "MachineLink lease does not grant Workspace writer ref control",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open Workspace writer ref stream"))?;
        let request_id = request_id("wwq")?;
        write_data_frame(
            &mut send,
            &WorkspaceWriterEnvelope {
                version: WORKSPACE_WRITER_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request: opts.request,
            },
        )
        .await?;
        let reply: WorkspaceWriterReply = read_data_frame(&mut recv).await?;
        if reply.version != WORKSPACE_WRITER_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
        {
            return Err(Error::Protocol(
                "Workspace writer response correlation failed; do not infer an unobserved ref mutation",
            ));
        }
        Ok::<_, Error>(WorkspaceWriterRequestResult {
            lease,
            response: reply.response,
        })
    });
    link.connection
        .close(0u32.into(), b"Workspace writer ref operation complete");
    link.endpoint
        .close(0u32.into(), b"Workspace writer ref operation complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_writer_operations_share_only_the_ref_capability() {
        let request = WorkspaceWriterRequest::Checkpoint {
            actor_user_id: "usr_fixture".into(),
            workspace_id: "ws_fixture".into(),
            writer_epoch: 2,
            authority_term: 1,
            session_id: "s_fixture".into(),
            branch_id: "b_fixture".into(),
            expected_head: "n_fixture".into(),
            expected_revision: 4,
            summary: "checkpoint".into(),
        };
        match request {
            WorkspaceWriterRequest::Checkpoint { writer_epoch, .. } => assert_eq!(writer_epoch, 2),
            _ => unreachable!(),
        }
        assert_ne!(
            MachineCapability::WorkspaceReplicaHandoff,
            MachineCapability::WorkspaceWriterRef
        );
    }
}
