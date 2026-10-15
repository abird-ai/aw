use super::reachability::{open_punched_quic, PunchOptions};
use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};

const MACHINE_LINK_VERSION: u16 = 1;
const MAX_MACHINE_CAPABILITIES: usize = 32;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MachineCapability {
    MachineInfo,
    WorkspaceDiscovery,
    FileRead,
    GitRead,
    SessionRead,
    Pty,
    RemoteSession,
    SemanticSyncRead,
    SemanticSyncWrite,
    RemoteSessionExecute,
    RemoteSessionStatus,
    RemoteSessionTranscript,
    RemoteSessionCancel,
    DomainAuthorityVote,
    DomainAuthorityReplicate,
    WorkspaceReplicaHandoff,
    WorkspaceWriterRef,
    IdentityContinuity,
    IdentityAuthenticate,
    ActorQuery,
    ActorInspect,
    ActorEnqueue,
    ActorStep,
    ActorControl,
    ActorNoticeRead,
}

impl MachineCapability {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "machine_info" | "machine-info" => Ok(Self::MachineInfo),
            "workspace_discovery" | "workspace-discovery" => Ok(Self::WorkspaceDiscovery),
            "file_read" | "file-read" => Ok(Self::FileRead),
            "git_read" | "git-read" => Ok(Self::GitRead),
            "session_read" | "session-read" => Ok(Self::SessionRead),
            "pty" => Ok(Self::Pty),
            "remote_session" | "remote-session" => Ok(Self::RemoteSession),
            "semantic_sync_read" | "semantic-sync-read" => Ok(Self::SemanticSyncRead),
            "semantic_sync_write" | "semantic-sync-write" => Ok(Self::SemanticSyncWrite),
            "remote_session_execute" | "remote-session-execute" => Ok(Self::RemoteSessionExecute),
            "remote_session_status" | "remote-session-status" => Ok(Self::RemoteSessionStatus),
            "remote_session_transcript" | "remote-session-transcript" => {
                Ok(Self::RemoteSessionTranscript)
            }
            "remote_session_cancel" | "remote-session-cancel" => Ok(Self::RemoteSessionCancel),
            "domain_authority_vote" | "domain-authority-vote" => Ok(Self::DomainAuthorityVote),
            "domain_authority_replicate" | "domain-authority-replicate" => {
                Ok(Self::DomainAuthorityReplicate)
            }
            "workspace_replica_handoff" | "workspace-replica-handoff" => {
                Ok(Self::WorkspaceReplicaHandoff)
            }
            "workspace_writer_ref" | "workspace-writer-ref" => Ok(Self::WorkspaceWriterRef),
            "identity_continuity" | "identity-continuity" => Ok(Self::IdentityContinuity),
            "identity_authenticate" | "identity-authenticate" => Ok(Self::IdentityAuthenticate),
            "actor_query" | "actor-query" => Ok(Self::ActorQuery),
            "actor_inspect" | "actor-inspect" => Ok(Self::ActorInspect),
            "actor_enqueue" | "actor-enqueue" => Ok(Self::ActorEnqueue),
            "actor_step" | "actor-step" => Ok(Self::ActorStep),
            "actor_control" | "actor-control" => Ok(Self::ActorControl),
            "actor_notice_read" | "actor-notice-read" => Ok(Self::ActorNoticeRead),
            _ => Err(Error::Config(format!(
                "unknown MachineLink capability {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MachineLinkServeOptions {
    pub route: PunchOptions,
    pub allowed: BTreeSet<MachineCapability>,
    pub lease: Duration,
}

#[derive(Clone, Debug)]
pub struct MachineLinkConnectOptions {
    pub route: PunchOptions,
    pub requested: BTreeSet<MachineCapability>,
    pub lease: Duration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineLinkHello {
    version: u16,
    node_id: String,
    peer_node_id: String,
    nonce: String,
    requested: Vec<MachineCapability>,
    lease_seconds: u64,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineLinkAccept {
    version: u16,
    node_id: String,
    peer_node_id: String,
    request_nonce: String,
    nonce: String,
    lease_id: String,
    expires_at: u64,
    granted: Vec<MachineCapability>,
    signature: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MachineLinkLease {
    pub peer_node_id: String,
    pub lease_id: String,
    pub expires_at: u64,
    pub capabilities: Vec<MachineCapability>,
    pub transport: &'static str,
}

pub(super) fn validate_caps(caps: &BTreeSet<MachineCapability>) -> Result<()> {
    if caps.is_empty() || caps.len() > MAX_MACHINE_CAPABILITIES {
        return Err(Error::Config(
            "MachineLink capability set must contain 1..=32 typed capabilities".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_lease(lease: Duration) -> Result<()> {
    if lease < Duration::from_secs(5) || lease > Duration::from_secs(300) {
        return Err(Error::Config(
            "MachineLink lease must be between 5 and 300 seconds".into(),
        ));
    }
    Ok(())
}

fn caps_bytes(caps: &[MachineCapability]) -> Result<Vec<u8>> {
    serde_json::to_vec(caps).map_err(|_| Error::Protocol("cannot encode MachineLink capabilities"))
}

fn hello_transcript(
    sender: &str,
    receiver: &str,
    nonce: &[u8; 32],
    requested: &[MachineCapability],
    lease_seconds: u64,
) -> Result<Vec<u8>> {
    let caps = caps_bytes(requested)?;
    Ok(transcript(
        b"aw-machine-link-hello-v1\0",
        &[
            sender.as_bytes(),
            receiver.as_bytes(),
            nonce,
            &caps,
            &lease_seconds.to_be_bytes(),
        ],
    ))
}

fn accept_transcript(
    sender: &str,
    receiver: &str,
    request_nonce: &[u8; 32],
    nonce: &[u8; 32],
    lease_id: &str,
    expires_at: u64,
    granted: &[MachineCapability],
) -> Result<Vec<u8>> {
    let caps = caps_bytes(granted)?;
    Ok(transcript(
        b"aw-machine-link-accept-v1\0",
        &[
            sender.as_bytes(),
            receiver.as_bytes(),
            request_nonce,
            nonce,
            lease_id.as_bytes(),
            &expires_at.to_be_bytes(),
            &caps,
        ],
    ))
}

fn random_lease_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for MachineLink lease"))?;
    Ok(format!("ml_{}", hex(&bytes)))
}

fn sorted_caps(caps: &BTreeSet<MachineCapability>) -> Vec<MachineCapability> {
    caps.iter().copied().collect()
}

pub(super) async fn negotiate_server(
    connection: &quinn::Connection,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    allowed: &BTreeSet<MachineCapability>,
    lease: Duration,
) -> Result<MachineLinkLease> {
    let (mut send, mut recv) =
        tokio::time::timeout(Duration::from_secs(10), connection.accept_bi())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("MachineLink control stream was not opened"))?;
    let hello: MachineLinkHello = read_frame(&mut recv).await?;
    if hello.version != MACHINE_LINK_VERSION
        || hello.node_id != peer.node_id
        || hello.peer_node_id != identity.node_id
    {
        return Err(Error::Protocol("MachineLink hello identity mismatch"));
    }
    if hello.requested.is_empty()
        || hello.requested.len() > MAX_MACHINE_CAPABILITIES
        || hello.lease_seconds == 0
        || hello.lease_seconds > lease.as_secs()
    {
        return Err(Error::Denied(
            "MachineLink request exceeds the server ceiling",
        ));
    }
    let requested: BTreeSet<_> = hello.requested.iter().copied().collect();
    if requested.len() != hello.requested.len() {
        return Err(Error::Protocol(
            "MachineLink request contains duplicate capabilities",
        ));
    }
    let request_nonce =
        decode_fixed::<32>(&hello.nonce, "MachineLink request nonce must be 32 bytes")?;
    let message = hello_transcript(
        &peer.node_id,
        &identity.node_id,
        &request_nonce,
        &hello.requested,
        hello.lease_seconds,
    )?;
    verify_signed_identity(&peer.node_id, &peer.public_key, &message, &hello.signature)?;
    let granted: Vec<_> = requested.intersection(allowed).copied().collect();
    if granted.is_empty() {
        return Err(Error::Denied(
            "MachineLink capability intersection is empty",
        ));
    }
    let lease_seconds = hello.lease_seconds.min(lease.as_secs());
    let expires_at = now_secs().saturating_add(lease_seconds);
    let lease_id = random_lease_id()?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for MachineLink accept"))?;
    let accept_message = accept_transcript(
        &identity.node_id,
        &peer.node_id,
        &request_nonce,
        &nonce,
        &lease_id,
        expires_at,
        &granted,
    )?;
    let accept = MachineLinkAccept {
        version: MACHINE_LINK_VERSION,
        node_id: identity.node_id.clone(),
        peer_node_id: peer.node_id.clone(),
        request_nonce: hello.nonce,
        nonce: STANDARD.encode(nonce),
        lease_id: lease_id.clone(),
        expires_at,
        granted: granted.clone(),
        signature: STANDARD.encode(identity.signing.sign(&accept_message).to_bytes()),
    };
    write_frame(&mut send, &accept).await?;
    Ok(MachineLinkLease {
        peer_node_id: peer.node_id.clone(),
        lease_id,
        expires_at,
        capabilities: granted,
        transport: "quic_machine_link",
    })
}

pub(super) async fn negotiate_client(
    connection: &quinn::Connection,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    requested_set: &BTreeSet<MachineCapability>,
    lease: Duration,
) -> Result<MachineLinkLease> {
    let requested = sorted_caps(requested_set);
    let lease_seconds = lease.as_secs();
    let (mut send, mut recv) = tokio::time::timeout(Duration::from_secs(10), connection.open_bi())
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Transport("cannot open MachineLink control stream"))?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for MachineLink hello"))?;
    let message = hello_transcript(
        &identity.node_id,
        &peer.node_id,
        &nonce,
        &requested,
        lease_seconds,
    )?;
    let hello = MachineLinkHello {
        version: MACHINE_LINK_VERSION,
        node_id: identity.node_id.clone(),
        peer_node_id: peer.node_id.clone(),
        nonce: STANDARD.encode(nonce),
        requested: requested.clone(),
        lease_seconds,
        signature: STANDARD.encode(identity.signing.sign(&message).to_bytes()),
    };
    write_frame(&mut send, &hello).await?;
    let accept: MachineLinkAccept = read_frame(&mut recv).await?;
    if accept.version != MACHINE_LINK_VERSION
        || accept.node_id != peer.node_id
        || accept.peer_node_id != identity.node_id
        || accept.expires_at <= now_secs()
        || accept.expires_at > now_secs().saturating_add(lease_seconds)
    {
        return Err(Error::Protocol("invalid MachineLink acceptance envelope"));
    }
    let echoed = decode_fixed::<32>(
        &accept.request_nonce,
        "MachineLink echoed nonce must be 32 bytes",
    )?;
    if echoed != nonce {
        return Err(Error::Protocol(
            "MachineLink acceptance is not bound to this request",
        ));
    }
    let accept_nonce =
        decode_fixed::<32>(&accept.nonce, "MachineLink accept nonce must be 32 bytes")?;
    let granted_set: BTreeSet<_> = accept.granted.iter().copied().collect();
    if granted_set.is_empty()
        || granted_set.len() != accept.granted.len()
        || !granted_set.is_subset(requested_set)
    {
        return Err(Error::Protocol(
            "MachineLink server granted invalid capabilities",
        ));
    }
    let accept_message = accept_transcript(
        &peer.node_id,
        &identity.node_id,
        &nonce,
        &accept_nonce,
        &accept.lease_id,
        accept.expires_at,
        &accept.granted,
    )?;
    verify_signed_identity(
        &peer.node_id,
        &peer.public_key,
        &accept_message,
        &accept.signature,
    )?;
    Ok(MachineLinkLease {
        peer_node_id: peer.node_id.clone(),
        lease_id: accept.lease_id,
        expires_at: accept.expires_at,
        capabilities: accept.granted,
        transport: "quic_machine_link",
    })
}

pub fn serve_machine_link(
    store: &SessionStore,
    state_dir: &Path,
    opts: MachineLinkServeOptions,
) -> Result<MachineLinkLease> {
    validate_caps(&opts.allowed)?;
    validate_lease(opts.lease)?;
    let expected_peer = opts.route.peer_node_id.clone();
    let peer = store.awnet_peer_by_node_id(&expected_peer)?;
    if peer.trust_state != "trusted" {
        return Err(Error::Denied("MachineLink requires a trusted peer"));
    }
    let identity = ensure_identity(store, state_dir)?;
    let link = open_punched_quic(store, state_dir, opts.route)?;
    let result = link.runtime.block_on(negotiate_server(
        &link.connection,
        &identity,
        &peer,
        &opts.allowed,
        opts.lease,
    ));
    link.connection
        .close(0u32.into(), b"MachineLink negotiation complete");
    link.endpoint
        .close(0u32.into(), b"MachineLink negotiation complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn connect_machine_link(
    store: &SessionStore,
    state_dir: &Path,
    opts: MachineLinkConnectOptions,
) -> Result<MachineLinkLease> {
    validate_caps(&opts.requested)?;
    validate_lease(opts.lease)?;
    let expected_peer = opts.route.peer_node_id.clone();
    let peer = store.awnet_peer_by_node_id(&expected_peer)?;
    if peer.trust_state != "trusted" {
        return Err(Error::Denied("MachineLink requires a trusted peer"));
    }
    let identity = ensure_identity(store, state_dir)?;
    let link = open_punched_quic(store, state_dir, opts.route)?;
    let result = link.runtime.block_on(negotiate_client(
        &link.connection,
        &identity,
        &peer,
        &opts.requested,
        opts.lease,
    ));
    link.connection
        .close(0u32.into(), b"MachineLink negotiation complete");
    link.endpoint
        .close(0u32.into(), b"MachineLink negotiation complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_names_are_typed_and_bounded() {
        assert_eq!(
            MachineCapability::parse("machine-info").unwrap(),
            MachineCapability::MachineInfo
        );
        assert!(MachineCapability::parse("shell").is_err());
        assert!(MachineCapability::parse("execute").is_err());
    }

    #[test]
    fn lease_bounds_are_strict() {
        assert!(validate_lease(Duration::from_secs(4)).is_err());
        assert!(validate_lease(Duration::from_secs(60)).is_ok());
        assert!(validate_lease(Duration::from_secs(301)).is_err());
    }
}
