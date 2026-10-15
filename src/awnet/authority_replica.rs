use super::machinelink::{
    negotiate_client, negotiate_server, MachineCapability, MachineLinkConnectOptions,
    MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_store::{
    domain_authority_snapshot_transcript, DomainAuthorityControlSnapshot,
    DomainAuthorityReplicaSnapshotInfo,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

const AUTHORITY_REPLICA_STREAM_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityReplicaRequest {
    authority_domain_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityReplicaEnvelope {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    request: AuthorityReplicaRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityReplicaReply {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    snapshot: DomainAuthorityControlSnapshot,
    source_signature: String,
}

#[derive(Clone, Debug)]
pub struct AuthorityReplicaServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
    pub authority_domain_id: String,
}

#[derive(Clone, Debug)]
pub struct AuthorityReplicaRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub actor: PrincipalId,
    pub authority_domain_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityReplicaServeResult {
    pub lease: MachineLinkLease,
    pub authority_domain_id: String,
    pub snapshot_sha256: String,
    pub through_commit_seq: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityReplicaRequestResult {
    pub lease: MachineLinkLease,
    pub replica: DomainAuthorityReplicaSnapshotInfo,
}

fn request_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for authority replica request"))?;
    Ok(format!("arq_{}", hex(&bytes)))
}

pub fn serve_authority_replica_snapshot(
    store: &SessionStore,
    state_dir: &Path,
    opts: AuthorityReplicaServeOptions,
) -> Result<AuthorityReplicaServeResult> {
    if !opts
        .link
        .allowed
        .contains(&MachineCapability::DomainAuthorityReplicate)
    {
        return Err(Error::Config(
            "authority replica server must explicitly grant domain-authority-replicate".into(),
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let snapshot =
        store.export_domain_authority_snapshot(&opts.actor, Some(&opts.authority_domain_id))?;
    if snapshot.source_node_id != identity.node_id {
        return Err(Error::Conflict(
            "Domain authority snapshot source does not match this AwNode identity",
        ));
    }
    if !snapshot.authority_set.members.contains(&peer.node_id) {
        return Err(Error::Denied(
            "authority replica peer is not in the active Domain AuthoritySet",
        ));
    }
    let source_signature = STANDARD.encode(
        identity
            .signing
            .sign(&domain_authority_snapshot_transcript(&snapshot))
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
        if !lease
            .capabilities
            .contains(&MachineCapability::DomainAuthorityReplicate)
        {
            return Err(Error::Denied(
                "MachineLink lease does not grant Domain authority replication",
            ));
        }
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("Domain authority replica stream was not opened"))?;
        let envelope: AuthorityReplicaEnvelope = read_data_frame(&mut recv).await?;
        if envelope.version != AUTHORITY_REPLICA_STREAM_VERSION
            || envelope.machine_lease_id != lease.lease_id
            || envelope.request.authority_domain_id != snapshot.authority_domain_id
        {
            return Err(Error::Protocol(
                "Domain authority replica request is not bound to the negotiated lease/domain",
            ));
        }
        write_data_frame(
            &mut send,
            &AuthorityReplicaReply {
                version: AUTHORITY_REPLICA_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id,
                snapshot: snapshot.clone(),
                source_signature: source_signature.clone(),
            },
        )
        .await?;
        Ok::<_, Error>(AuthorityReplicaServeResult {
            lease,
            authority_domain_id: snapshot.authority_domain_id.clone(),
            snapshot_sha256: snapshot.snapshot_sha256.clone(),
            through_commit_seq: snapshot.through_commit_seq,
        })
    });
    link.connection
        .close(0u32.into(), b"Domain authority replica complete");
    link.endpoint
        .close(0u32.into(), b"Domain authority replica complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_authority_replica_snapshot(
    store: &SessionStore,
    state_dir: &Path,
    opts: AuthorityReplicaRequestOptions,
) -> Result<AuthorityReplicaRequestResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::DomainAuthorityReplicate)
    {
        return Err(Error::Config(
            "authority replica client must explicitly request domain-authority-replicate".into(),
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
        if !lease
            .capabilities
            .contains(&MachineCapability::DomainAuthorityReplicate)
        {
            return Err(Error::Denied(
                "MachineLink lease does not grant Domain authority replication",
            ));
        }
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open Domain authority replica stream"))?;
        let request_id = request_id()?;
        write_data_frame(
            &mut send,
            &AuthorityReplicaEnvelope {
                version: AUTHORITY_REPLICA_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request: AuthorityReplicaRequest {
                    authority_domain_id: opts.authority_domain_id.clone(),
                },
            },
        )
        .await?;
        let reply: AuthorityReplicaReply = read_data_frame(&mut recv).await?;
        if reply.version != AUTHORITY_REPLICA_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
            || reply.snapshot.authority_domain_id != opts.authority_domain_id
            || reply.snapshot.source_node_id != peer.node_id
        {
            return Err(Error::Protocol(
                "Domain authority replica response correlation failed",
            ));
        }
        verify_signed_identity(
            &peer.node_id,
            &peer.public_key,
            &domain_authority_snapshot_transcript(&reply.snapshot),
            &reply.source_signature,
        )?;
        let replica = store.import_domain_authority_replica_snapshot(
            &opts.actor,
            &peer.node_id,
            &reply.snapshot,
            &reply.source_signature,
        )?;
        Ok::<_, Error>(AuthorityReplicaRequestResult { lease, replica })
    });
    link.connection
        .close(0u32.into(), b"Domain authority replica complete");
    link.endpoint
        .close(0u32.into(), b"Domain authority replica complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}
