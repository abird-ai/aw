use super::machinelink::{
    negotiate_client, negotiate_server, MachineCapability, MachineLinkConnectOptions,
    MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_store::{
    domain_authority_vote_transcript, DomainAuthorityElectionInfo, DomainAuthorityPeerVoteInfo,
    DomainAuthorityReplicaElectionInfo,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

const AUTHORITY_STREAM_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityVoteRequest {
    domain_id: String,
    set_revision: u64,
    members_sha256: String,
    quorum: u64,
    term: u64,
    election_id: String,
    candidate_node_id: String,
    ttl_seconds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityVoteReply {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    vote: DomainAuthorityPeerVoteInfo,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityVoteEnvelope {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    request: AuthorityVoteRequest,
}

#[derive(Clone, Debug)]
pub struct AuthorityVoteServeOptions {
    pub link: MachineLinkServeOptions,
}

#[derive(Clone, Debug)]
pub struct AuthorityVoteRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub actor: PrincipalId,
    pub election_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityVoteServeResult {
    pub lease: MachineLinkLease,
    pub vote: DomainAuthorityPeerVoteInfo,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityVoteRequestResult {
    pub lease: MachineLinkLease,
    pub election: DomainAuthorityElectionInfo,
    pub voter_node_id: String,
    pub vote_expires_at: i64,
}

#[derive(Clone, Debug)]
pub struct AuthorityReplicaVoteRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub actor: PrincipalId,
    pub election_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityReplicaVoteRequestResult {
    pub lease: MachineLinkLease,
    pub election: DomainAuthorityReplicaElectionInfo,
    pub voter_node_id: String,
    pub vote_expires_at: i64,
}

fn request_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for authority vote request"))?;
    Ok(format!("avq_{}", hex(&bytes)))
}

pub fn serve_authority_vote(
    store: &SessionStore,
    state_dir: &Path,
    opts: AuthorityVoteServeOptions,
) -> Result<AuthorityVoteServeResult> {
    if !opts
        .link
        .allowed
        .contains(&MachineCapability::DomainAuthorityVote)
    {
        return Err(Error::Config(
            "authority vote server must explicitly grant domain-authority-vote".into(),
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
        if !lease
            .capabilities
            .contains(&MachineCapability::DomainAuthorityVote)
        {
            return Err(Error::Denied(
                "MachineLink lease does not grant Domain authority voting",
            ));
        }
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("Domain authority vote stream was not opened"))?;
        let envelope: AuthorityVoteEnvelope = read_data_frame(&mut recv).await?;
        if envelope.version != AUTHORITY_STREAM_VERSION
            || envelope.machine_lease_id != lease.lease_id
        {
            return Err(Error::Protocol(
                "Domain authority vote is not bound to the MachineLink lease",
            ));
        }
        let request = envelope.request;
        if request.candidate_node_id != peer.node_id {
            return Err(Error::Denied(
                "Domain authority candidate does not match the authenticated peer",
            ));
        }
        let vote = store.prepare_domain_authority_peer_vote(
            &peer.node_id,
            &request.domain_id,
            request.set_revision,
            &request.members_sha256,
            request.quorum,
            request.term,
            &request.election_id,
            &request.candidate_node_id,
            request.ttl_seconds,
        )?;
        let signature = STANDARD.encode(
            identity
                .signing
                .sign(&domain_authority_vote_transcript(&vote))
                .to_bytes(),
        );
        write_data_frame(
            &mut send,
            &AuthorityVoteReply {
                version: AUTHORITY_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id,
                vote: vote.clone(),
                signature,
            },
        )
        .await?;
        Ok::<_, Error>(AuthorityVoteServeResult { lease, vote })
    });
    link.connection
        .close(0u32.into(), b"Domain authority vote complete");
    link.endpoint
        .close(0u32.into(), b"Domain authority vote complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_authority_replica_vote(
    store: &SessionStore,
    state_dir: &Path,
    opts: AuthorityReplicaVoteRequestOptions,
) -> Result<AuthorityReplicaVoteRequestResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::DomainAuthorityVote)
    {
        return Err(Error::Config(
            "authority replica vote client must explicitly request domain-authority-vote".into(),
        ));
    }
    let election = store.domain_authority_replica_election(&opts.actor, &opts.election_id)?;
    if election.state != "collecting" {
        return Err(Error::Conflict(
            "Domain authority replica election is not collecting votes",
        ));
    }
    let replica = store.domain_authority_replica(&opts.actor, &election.authority_domain_id)?;
    let set = &replica.snapshot.authority_set;
    if set.set_revision != election.set_revision
        || set.members_sha256 != election.members_sha256
        || set.quorum != election.quorum
    {
        return Err(Error::Conflict(
            "replicated AuthoritySet changed while promotion election was collecting",
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    if !set.members.contains(&peer.node_id) {
        return Err(Error::Denied(
            "requested voter is not in the replicated AuthoritySet",
        ));
    }
    let identity = ensure_identity(store, state_dir)?;
    if election.candidate_node_id != identity.node_id {
        return Err(Error::Denied(
            "this AwNode is not the candidate for the standby promotion election",
        ));
    }
    let request = AuthorityVoteRequest {
        domain_id: election.authority_domain_id.clone(),
        set_revision: election.set_revision,
        members_sha256: election.members_sha256.clone(),
        quorum: election.quorum,
        term: election.term,
        election_id: election.id.clone(),
        candidate_node_id: election.candidate_node_id.clone(),
        ttl_seconds: election.requested_ttl_seconds,
    };
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
            .contains(&MachineCapability::DomainAuthorityVote)
        {
            return Err(Error::Denied(
                "MachineLink lease does not grant Domain authority voting",
            ));
        }
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open Domain authority vote stream"))?;
        let request_id = request_id()?;
        write_data_frame(
            &mut send,
            &AuthorityVoteEnvelope {
                version: AUTHORITY_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request,
            },
        )
        .await?;
        let reply: AuthorityVoteReply = read_data_frame(&mut recv).await?;
        if reply.version != AUTHORITY_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
        {
            return Err(Error::Protocol(
                "Domain authority replica vote response correlation failed",
            ));
        }
        if reply.vote.voter_node_id != peer.node_id
            || reply.vote.authority_domain_id != election.authority_domain_id
            || reply.vote.set_revision != election.set_revision
            || reply.vote.members_sha256 != election.members_sha256
            || reply.vote.term != election.term
            || reply.vote.election_id != election.id
            || reply.vote.candidate_node_id != election.candidate_node_id
            || reply.vote.ttl_seconds != election.requested_ttl_seconds
        {
            return Err(Error::Protocol(
                "Domain authority replica vote does not match the requested election",
            ));
        }
        verify_signed_identity(
            &peer.node_id,
            &peer.public_key,
            &domain_authority_vote_transcript(&reply.vote),
            &reply.signature,
        )?;
        let updated = store.record_domain_authority_replica_vote(
            &opts.actor,
            &election.id,
            &peer.node_id,
            reply.vote.expires_at,
            &reply.signature,
        )?;
        Ok::<_, Error>(AuthorityReplicaVoteRequestResult {
            lease,
            election: updated,
            voter_node_id: peer.node_id.clone(),
            vote_expires_at: reply.vote.expires_at,
        })
    });
    link.connection
        .close(0u32.into(), b"Domain authority replica vote complete");
    link.endpoint
        .close(0u32.into(), b"Domain authority replica vote complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_authority_vote(
    store: &SessionStore,
    state_dir: &Path,
    opts: AuthorityVoteRequestOptions,
) -> Result<AuthorityVoteRequestResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::DomainAuthorityVote)
    {
        return Err(Error::Config(
            "authority vote client must explicitly request domain-authority-vote".into(),
        ));
    }
    let election = store.domain_authority_election(&opts.actor, &opts.election_id)?;
    if election.state != "collecting" {
        return Err(Error::Conflict(
            "Domain authority election is not collecting votes",
        ));
    }
    let set = store.domain_authority_set(&opts.actor, Some(&election.domain_id))?;
    if set.set_revision != election.set_revision
        || set.members_sha256 != election.members_sha256
        || set.quorum != election.quorum
    {
        return Err(Error::Conflict(
            "AuthoritySet changed while election was collecting",
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    if !set.members.contains(&peer.node_id) {
        return Err(Error::Denied(
            "requested voter is not in the active Domain AuthoritySet",
        ));
    }
    let identity = ensure_identity(store, state_dir)?;
    if election.candidate_node_id != identity.node_id {
        return Err(Error::Denied(
            "this AwNode is not the candidate for the Domain authority election",
        ));
    }
    let request = AuthorityVoteRequest {
        domain_id: election.domain_id.clone(),
        set_revision: election.set_revision,
        members_sha256: election.members_sha256.clone(),
        quorum: election.quorum,
        term: election.term,
        election_id: election.id.clone(),
        candidate_node_id: election.candidate_node_id.clone(),
        ttl_seconds: election.requested_ttl_seconds,
    };
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
            .contains(&MachineCapability::DomainAuthorityVote)
        {
            return Err(Error::Denied(
                "MachineLink lease does not grant Domain authority voting",
            ));
        }
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open Domain authority vote stream"))?;
        let request_id = request_id()?;
        write_data_frame(
            &mut send,
            &AuthorityVoteEnvelope {
                version: AUTHORITY_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request,
            },
        )
        .await?;
        let reply: AuthorityVoteReply = read_data_frame(&mut recv).await?;
        if reply.version != AUTHORITY_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
        {
            return Err(Error::Protocol(
                "Domain authority vote response correlation failed",
            ));
        }
        if reply.vote.voter_node_id != peer.node_id
            || reply.vote.authority_domain_id != election.domain_id
            || reply.vote.set_revision != election.set_revision
            || reply.vote.members_sha256 != election.members_sha256
            || reply.vote.term != election.term
            || reply.vote.election_id != election.id
            || reply.vote.candidate_node_id != election.candidate_node_id
            || reply.vote.ttl_seconds != election.requested_ttl_seconds
        {
            return Err(Error::Protocol(
                "Domain authority vote does not match the requested election",
            ));
        }
        verify_signed_identity(
            &peer.node_id,
            &peer.public_key,
            &domain_authority_vote_transcript(&reply.vote),
            &reply.signature,
        )?;
        let updated = store.record_domain_authority_vote(
            &opts.actor,
            &election.id,
            &peer.node_id,
            reply.vote.expires_at,
            &reply.signature,
        )?;
        Ok::<_, Error>(AuthorityVoteRequestResult {
            lease,
            election: updated,
            voter_node_id: peer.node_id.clone(),
            vote_expires_at: reply.vote.expires_at,
        })
    });
    link.connection
        .close(0u32.into(), b"Domain authority vote complete");
    link.endpoint
        .close(0u32.into(), b"Domain authority vote complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}
