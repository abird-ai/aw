use super::*;
use quinn::EndpointConfig;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const STUN_MAGIC: u32 = 0x2112_A442;
const STUN_BINDING_REQUEST: u16 = 0x0001;
const STUN_BINDING_SUCCESS: u16 = 0x0101;
const STUN_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const RENDEZVOUS_VERSION: u16 = 1;
const ROUTE_VERSION: u16 = 1;
const MAX_RENDEZVOUS_PACKET: usize = 16 * 1024;
const MAX_RENDEZVOUS_PACKETS: usize = 256;
const MAX_RENDEZVOUS_SERVICE_PACKETS: usize = 100_000;
const RENDEZVOUS_RETRY: Duration = Duration::from_millis(250);
const PUNCH_ROUNDS: usize = 8;
const PUNCH_DELAY: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    Host,
    ServerReflexive,
    RendezvousObserved,
    Relay,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RouteCandidate {
    pub kind: CandidateKind,
    pub address: SocketAddr,
    pub priority: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReachabilityReport {
    pub bound: SocketAddr,
    pub candidates: Vec<RouteCandidate>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RendezvousBound {
    pub address: SocketAddr,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RendezvousMatched {
    pub room: String,
    pub first_node: String,
    pub second_node: String,
}

#[derive(Clone, Debug)]
pub struct RendezvousOptions {
    pub listen: SocketAddr,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct RendezvousServiceOptions {
    pub listen: SocketAddr,
    pub lifetime: Duration,
    pub room_ttl: Duration,
    pub max_rooms: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct RendezvousServiceStats {
    pub address: SocketAddr,
    pub registrations: u64,
    pub rooms_created: u64,
    pub pairs_matched: u64,
    pub rooms_expired: u64,
    pub invalid_packets: u64,
    pub active_rooms_at_shutdown: usize,
}

#[derive(Clone, Debug)]
pub struct PunchOptions {
    pub peer_node_id: String,
    pub room: String,
    pub rendezvous: SocketAddr,
    pub listen: SocketAddr,
    pub stun: Vec<SocketAddr>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirectRoute {
    pub peer_node_id: String,
    pub local_addr: SocketAddr,
    pub peer_addr: SocketAddr,
    pub transport: &'static str,
    pub candidate_kind: CandidateKind,
    pub role: &'static str,
    pub discovered_candidates: Vec<RouteCandidate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RendezvousRegistration {
    pub(super) version: u16,
    pub(super) room: String,
    pub(super) node_id: String,
    pub(super) public_key: String,
    pub(super) cert_der: String,
    pub(super) nonce: String,
    pub(super) expires_at: u64,
    pub(super) candidates: Vec<RouteCandidate>,
    pub(super) signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RendezvousClientPacket {
    Register {
        registration: RendezvousRegistration,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)] // bounded wire packet; avoid an allocation on every peer response
enum RendezvousServerPacket {
    Peer {
        room: String,
        peer_addr: String,
        peer: RendezvousRegistration,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteHello {
    version: u16,
    room: String,
    node_id: String,
    nonce: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteAck {
    version: u16,
    room: String,
    node_id: String,
    request_nonce: String,
    nonce: String,
    signature: String,
}

pub(super) fn validate_room(room: &str) -> Result<()> {
    if room.len() != 64
        || !room
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Config(
            "rendezvous room must be exactly 32 bytes of lowercase hex".into(),
        ));
    }
    Ok(())
}

pub fn new_room() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for rendezvous room"))?;
    Ok(hex(&bytes))
}

fn stun_request() -> Result<([u8; 20], [u8; 12])> {
    let mut tx = [0u8; 12];
    getrandom::fill(&mut tx)
        .map_err(|_| Error::Storage("OS randomness unavailable for STUN transaction"))?;
    let mut request = [0u8; 20];
    request[0..2].copy_from_slice(&STUN_BINDING_REQUEST.to_be_bytes());
    request[2..4].copy_from_slice(&0u16.to_be_bytes());
    request[4..8].copy_from_slice(&STUN_MAGIC.to_be_bytes());
    request[8..20].copy_from_slice(&tx);
    Ok((request, tx))
}

fn parse_stun_address(bytes: &[u8], tx: &[u8; 12]) -> Result<SocketAddr> {
    if bytes.len() < 20
        || u16::from_be_bytes([bytes[0], bytes[1]]) != STUN_BINDING_SUCCESS
        || u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) != STUN_MAGIC
        || bytes[8..20] != tx[..]
    {
        return Err(Error::Protocol("invalid STUN binding response"));
    }
    let declared = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
    if declared + 20 > bytes.len() {
        return Err(Error::Protocol("truncated STUN binding response"));
    }
    let mut offset = 20usize;
    let end = 20 + declared;
    while offset + 4 <= end {
        let ty = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        let len = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        offset += 4;
        if offset + len > end {
            return Err(Error::Protocol("truncated STUN attribute"));
        }
        let value = &bytes[offset..offset + len];
        if ty == STUN_XOR_MAPPED_ADDRESS {
            if value.len() < 8 || value[0] != 0 {
                return Err(Error::Protocol("invalid STUN XOR-MAPPED-ADDRESS"));
            }
            let port = u16::from_be_bytes([value[2], value[3]]) ^ ((STUN_MAGIC >> 16) as u16);
            match value[1] {
                0x01 if value.len() == 8 => {
                    let mask = STUN_MAGIC.to_be_bytes();
                    let mut ip = [0u8; 4];
                    for i in 0..4 {
                        ip[i] = value[4 + i] ^ mask[i];
                    }
                    return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port));
                }
                0x02 if value.len() == 20 => {
                    let mut mask = [0u8; 16];
                    mask[..4].copy_from_slice(&STUN_MAGIC.to_be_bytes());
                    mask[4..].copy_from_slice(tx);
                    let mut ip = [0u8; 16];
                    for i in 0..16 {
                        ip[i] = value[4 + i] ^ mask[i];
                    }
                    return Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(ip)), port));
                }
                _ => return Err(Error::Protocol("unsupported STUN address family")),
            }
        }
        offset += (len + 3) & !3;
    }
    Err(Error::Protocol(
        "STUN response did not contain XOR-MAPPED-ADDRESS",
    ))
}

pub fn stun_binding(
    socket: &UdpSocket,
    server: SocketAddr,
    timeout: Duration,
) -> Result<SocketAddr> {
    let (request, tx) = stun_request()?;
    let previous = socket.read_timeout()?;
    socket.set_read_timeout(Some(timeout))?;
    let result = (|| {
        socket
            .send_to(&request, server)
            .map_err(|_| Error::Transport("failed to send STUN binding request"))?;
        let mut response = [0u8; 2048];
        let (len, source) = socket
            .recv_from(&mut response)
            .map_err(|_| Error::Transport("STUN binding request timed out or failed"))?;
        if source != server {
            return Err(Error::Protocol(
                "STUN response came from an unexpected server",
            ));
        }
        parse_stun_address(&response[..len], &tx)
    })();
    let _ = socket.set_read_timeout(previous);
    result
}

fn dedup_candidates(candidates: &mut Vec<RouteCandidate>) {
    candidates.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.address.to_string().cmp(&b.address.to_string()))
    });
    candidates.dedup_by(|a, b| a.address == b.address);
}

pub fn gather_candidates(
    socket: &UdpSocket,
    stun_servers: &[SocketAddr],
) -> Result<ReachabilityReport> {
    let bound = socket.local_addr()?;
    let mut candidates = Vec::new();
    let host = discover_advertise(bound);
    if !host.ip().is_unspecified() && host.port() != 0 {
        candidates.push(RouteCandidate {
            kind: CandidateKind::Host,
            address: host,
            priority: 2_000_000,
        });
    }
    for server in stun_servers.iter().take(4) {
        if let Ok(address) = stun_binding(socket, *server, Duration::from_millis(900)) {
            if address.port() != 0 && !address.ip().is_unspecified() {
                candidates.push(RouteCandidate {
                    kind: CandidateKind::ServerReflexive,
                    address,
                    priority: 1_500_000,
                });
            }
        }
    }
    dedup_candidates(&mut candidates);
    Ok(ReachabilityReport { bound, candidates })
}

fn registration_transcript(
    room: &str,
    node_id: &str,
    cert_der: &str,
    candidates: &[RouteCandidate],
    nonce: &[u8; 32],
    expires_at: u64,
) -> Result<Vec<u8>> {
    let candidate_bytes = serde_json::to_vec(candidates)
        .map_err(|_| Error::Protocol("cannot encode route candidates"))?;
    let cert = STANDARD
        .decode(cert_der)
        .map_err(|_| Error::Protocol("invalid route certificate"))?;
    let cert_hash = Sha256::digest(&cert);
    Ok(transcript(
        b"aw-rendezvous-registration-v1\0",
        &[
            room.as_bytes(),
            node_id.as_bytes(),
            &cert_hash,
            &candidate_bytes,
            nonce,
            &expires_at.to_be_bytes(),
        ],
    ))
}

pub(super) fn validate_registration_shape(registration: &RendezvousRegistration) -> Result<()> {
    if registration.version != RENDEZVOUS_VERSION {
        return Err(Error::Protocol("unsupported rendezvous registration"));
    }
    validate_room(&registration.room)?;
    if registration.expires_at <= now_secs()
        || registration.expires_at > now_secs().saturating_add(900)
    {
        return Err(Error::Denied(
            "rendezvous registration expired or too long-lived",
        ));
    }
    let public = decode_hex_fixed::<32>(
        &registration.public_key,
        "rendezvous public key must be 32-byte hex",
    )?;
    if node_id_from_public_key(&public) != registration.node_id {
        return Err(Error::Protocol("rendezvous Node ID/public key mismatch"));
    }
    let _ = decode_fixed::<32>(&registration.nonce, "rendezvous nonce must be 32 bytes")?;
    let cert = STANDARD
        .decode(&registration.cert_der)
        .map_err(|_| Error::Protocol("rendezvous certificate is not valid base64"))?;
    if cert.is_empty() || cert.len() > 16 * 1024 || registration.candidates.len() > 12 {
        return Err(Error::Limit(
            "rendezvous registration exceeds bounded shape",
        ));
    }
    for candidate in &registration.candidates {
        if candidate.address.port() == 0 || candidate.address.ip().is_unspecified() {
            return Err(Error::Protocol("rendezvous candidate is unusable"));
        }
    }
    Ok(())
}

fn validate_registration_signature(registration: &RendezvousRegistration) -> Result<()> {
    validate_registration_shape(registration)?;
    let nonce = decode_fixed::<32>(&registration.nonce, "rendezvous nonce must be 32 bytes")?;
    let message = registration_transcript(
        &registration.room,
        &registration.node_id,
        &registration.cert_der,
        &registration.candidates,
        &nonce,
        registration.expires_at,
    )?;
    verify_signed_identity(
        &registration.node_id,
        &registration.public_key,
        &message,
        &registration.signature,
    )
}

pub(super) fn validate_peer_registration(
    store: &SessionStore,
    expected_peer: &str,
    registration: &RendezvousRegistration,
) -> Result<()> {
    validate_registration_shape(registration)?;
    if registration.node_id != expected_peer {
        return Err(Error::Denied(
            "rendezvous returned an unexpected peer identity",
        ));
    }
    let trusted = store.awnet_peer_by_node_id(expected_peer)?;
    if trusted.public_key != registration.public_key || trusted.trust_state != "trusted" {
        return Err(Error::Denied("rendezvous peer is not currently trusted"));
    }
    let nonce = decode_fixed::<32>(&registration.nonce, "rendezvous nonce must be 32 bytes")?;
    let message = registration_transcript(
        &registration.room,
        &registration.node_id,
        &registration.cert_der,
        &registration.candidates,
        &nonce,
        registration.expires_at,
    )?;
    verify_signed_identity(
        &registration.node_id,
        &registration.public_key,
        &message,
        &registration.signature,
    )
}

pub(super) fn build_registration(
    identity: &NodeIdentity,
    room: &str,
    candidates: Vec<RouteCandidate>,
    timeout: Duration,
) -> Result<RendezvousRegistration> {
    validate_room(room)?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for rendezvous registration"))?;
    let expires_at = now_secs().saturating_add(timeout.as_secs().min(900));
    let cert_der = STANDARD.encode(&identity.quic_cert_der);
    let message = registration_transcript(
        room,
        &identity.node_id,
        &cert_der,
        &candidates,
        &nonce,
        expires_at,
    )?;
    let signature = identity.signing.sign(&message);
    Ok(RendezvousRegistration {
        version: RENDEZVOUS_VERSION,
        room: room.to_owned(),
        node_id: identity.node_id.clone(),
        public_key: identity.public_key.clone(),
        cert_der,
        nonce: STANDARD.encode(nonce),
        expires_at,
        candidates,
        signature: STANDARD.encode(signature.to_bytes()),
    })
}

pub fn serve_rendezvous<F>(opts: RendezvousOptions, announce: F) -> Result<RendezvousMatched>
where
    F: FnOnce(&RendezvousBound) -> Result<()>,
{
    if opts.timeout < Duration::from_secs(5) || opts.timeout > Duration::from_secs(900) {
        return Err(Error::Config(
            "rendezvous timeout must be between 5 and 900 seconds".into(),
        ));
    }
    runtime()?.block_on(async move {
        let socket = tokio::net::UdpSocket::bind(opts.listen)
            .await
            .map_err(|_| Error::Transport("cannot bind temporary rendezvous UDP socket"))?;
        let address = socket
            .local_addr()
            .map_err(|_| Error::Transport("cannot inspect rendezvous address"))?;
        announce(&RendezvousBound {
            address,
            expires_at: now_secs().saturating_add(opts.timeout.as_secs()),
        })?;
        let deadline = tokio::time::Instant::now() + opts.timeout;
        let mut rooms: HashMap<String, (SocketAddr, RendezvousRegistration)> = HashMap::new();
        let mut buffer = vec![0u8; MAX_RENDEZVOUS_PACKET];
        for _ in 0..MAX_RENDEZVOUS_PACKETS {
            let received = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer))
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("temporary rendezvous receive failed"))?;
            let (len, source) = received;
            let packet: RendezvousClientPacket = match serde_json::from_slice(&buffer[..len]) {
                Ok(packet) => packet,
                Err(_) => continue,
            };
            let RendezvousClientPacket::Register { registration } = packet;
            if validate_registration_signature(&registration).is_err() {
                continue;
            }
            if let Some((first_addr, first)) = rooms.get(&registration.room).cloned() {
                if first.node_id == registration.node_id {
                    rooms.insert(registration.room.clone(), (source, registration));
                    continue;
                }
                let first_reply = RendezvousServerPacket::Peer {
                    room: registration.room.clone(),
                    peer_addr: source.to_string(),
                    peer: registration.clone(),
                };
                let second_reply = RendezvousServerPacket::Peer {
                    room: registration.room.clone(),
                    peer_addr: first_addr.to_string(),
                    peer: first.clone(),
                };
                let first_bytes = serde_json::to_vec(&first_reply)
                    .map_err(|_| Error::Protocol("cannot encode rendezvous response"))?;
                let second_bytes = serde_json::to_vec(&second_reply)
                    .map_err(|_| Error::Protocol("cannot encode rendezvous response"))?;
                if first_bytes.len() > MAX_RENDEZVOUS_PACKET
                    || second_bytes.len() > MAX_RENDEZVOUS_PACKET
                {
                    return Err(Error::Limit("rendezvous response exceeds packet bound"));
                }
                socket
                    .send_to(&first_bytes, first_addr)
                    .await
                    .map_err(|_| Error::Transport("temporary rendezvous send failed"))?;
                socket
                    .send_to(&second_bytes, source)
                    .await
                    .map_err(|_| Error::Transport("temporary rendezvous send failed"))?;
                return Ok(RendezvousMatched {
                    room: registration.room,
                    first_node: first.node_id,
                    second_node: registration.node_id,
                });
            }
            rooms.insert(registration.room.clone(), (source, registration));
            rooms.retain(|_, (_, entry)| entry.expires_at > now_secs());
            if rooms.len() > 128 {
                return Err(Error::Limit("temporary rendezvous room limit reached"));
            }
        }
        Err(Error::Denied(
            "temporary rendezvous packet budget exhausted",
        ))
    })
}

pub fn serve_rendezvous_service<F>(
    opts: RendezvousServiceOptions,
    announce: F,
) -> Result<RendezvousServiceStats>
where
    F: FnOnce(&RendezvousBound) -> Result<()>,
{
    if opts.lifetime < Duration::from_secs(5) || opts.lifetime > Duration::from_secs(86_400) {
        return Err(Error::Config(
            "rendezvous service lifetime must be between 5 seconds and 24 hours".into(),
        ));
    }
    if opts.room_ttl < Duration::from_secs(3)
        || opts.room_ttl > Duration::from_secs(900)
        || opts.max_rooms == 0
        || opts.max_rooms > 4096
    {
        return Err(Error::Config(
            "rendezvous service requires room TTL 3..900 seconds and max_rooms 1..4096".into(),
        ));
    }
    runtime()?.block_on(async move {
        let socket = tokio::net::UdpSocket::bind(opts.listen)
            .await
            .map_err(|_| Error::Transport("cannot bind reusable rendezvous UDP socket"))?;
        let address = socket
            .local_addr()
            .map_err(|_| Error::Transport("cannot inspect reusable rendezvous address"))?;
        announce(&RendezvousBound {
            address,
            expires_at: now_secs().saturating_add(opts.lifetime.as_secs()),
        })?;
        let deadline = tokio::time::Instant::now() + opts.lifetime;
        let mut rooms: HashMap<String, (SocketAddr, RendezvousRegistration, u64)> = HashMap::new();
        let mut buffer = vec![0u8; MAX_RENDEZVOUS_PACKET];
        let mut registrations = 0u64;
        let mut rooms_created = 0u64;
        let mut pairs_matched = 0u64;
        let mut rooms_expired = 0u64;
        let mut invalid_packets = 0u64;

        for _ in 0..MAX_RENDEZVOUS_SERVICE_PACKETS {
            let now_epoch = now_secs();
            let before = rooms.len();
            rooms.retain(|_, (_, entry, expires)| {
                *expires > now_epoch && entry.expires_at > now_epoch
            });
            rooms_expired = rooms_expired.saturating_add((before - rooms.len()) as u64);
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let received = match tokio::time::timeout(
                remaining.min(Duration::from_secs(1)),
                socket.recv_from(&mut buffer),
            )
            .await
            {
                Ok(Ok(value)) => value,
                Ok(Err(_)) => return Err(Error::Transport("reusable rendezvous receive failed")),
                Err(_) => {
                    if tokio::time::Instant::now() >= deadline {
                        break;
                    }
                    continue;
                }
            };
            let (len, source) = received;
            let packet: RendezvousClientPacket = match serde_json::from_slice(&buffer[..len]) {
                Ok(packet) => packet,
                Err(_) => {
                    invalid_packets = invalid_packets.saturating_add(1);
                    continue;
                }
            };
            let RendezvousClientPacket::Register { registration } = packet;
            if validate_registration_signature(&registration).is_err() {
                invalid_packets = invalid_packets.saturating_add(1);
                continue;
            }
            registrations = registrations.saturating_add(1);
            if let Some((first_addr, first, _)) = rooms.remove(&registration.room) {
                if first.node_id == registration.node_id {
                    let expires = registration
                        .expires_at
                        .min(now_secs().saturating_add(opts.room_ttl.as_secs()));
                    rooms.insert(registration.room.clone(), (source, registration, expires));
                    continue;
                }
                let first_reply = RendezvousServerPacket::Peer {
                    room: registration.room.clone(),
                    peer_addr: source.to_string(),
                    peer: registration.clone(),
                };
                let second_reply = RendezvousServerPacket::Peer {
                    room: registration.room.clone(),
                    peer_addr: first_addr.to_string(),
                    peer: first.clone(),
                };
                let first_bytes = serde_json::to_vec(&first_reply)
                    .map_err(|_| Error::Protocol("cannot encode reusable rendezvous response"))?;
                let second_bytes = serde_json::to_vec(&second_reply)
                    .map_err(|_| Error::Protocol("cannot encode reusable rendezvous response"))?;
                if first_bytes.len() > MAX_RENDEZVOUS_PACKET
                    || second_bytes.len() > MAX_RENDEZVOUS_PACKET
                {
                    return Err(Error::Limit(
                        "reusable rendezvous response exceeds packet bound",
                    ));
                }
                socket
                    .send_to(&first_bytes, first_addr)
                    .await
                    .map_err(|_| Error::Transport("reusable rendezvous send failed"))?;
                socket
                    .send_to(&second_bytes, source)
                    .await
                    .map_err(|_| Error::Transport("reusable rendezvous send failed"))?;
                pairs_matched = pairs_matched.saturating_add(1);
                continue;
            }
            if rooms.len() >= opts.max_rooms {
                let denied = serde_json::to_vec(&RendezvousServerPacket::Error {
                    message: "rendezvous room capacity reached".into(),
                })
                .map_err(|_| Error::Protocol("cannot encode rendezvous capacity response"))?;
                let _ = socket.send_to(&denied, source).await;
                continue;
            }
            let expires = registration
                .expires_at
                .min(now_secs().saturating_add(opts.room_ttl.as_secs()));
            rooms.insert(registration.room.clone(), (source, registration, expires));
            rooms_created = rooms_created.saturating_add(1);
        }
        Ok(RendezvousServiceStats {
            address,
            registrations,
            rooms_created,
            pairs_matched,
            rooms_expired,
            invalid_packets,
            active_rooms_at_shutdown: rooms.len(),
        })
    })
}

fn rendezvous_exchange(
    socket: &UdpSocket,
    rendezvous: SocketAddr,
    registration: &RendezvousRegistration,
    timeout: Duration,
) -> Result<(SocketAddr, RendezvousRegistration)> {
    let packet = serde_json::to_vec(&RendezvousClientPacket::Register {
        registration: registration.clone(),
    })
    .map_err(|_| Error::Protocol("cannot encode rendezvous registration"))?;
    if packet.len() > MAX_RENDEZVOUS_PACKET {
        return Err(Error::Limit("rendezvous registration exceeds packet bound"));
    }
    let previous = socket.read_timeout()?;
    socket.set_read_timeout(Some(RENDEZVOUS_RETRY))?;
    let deadline = Instant::now() + timeout;
    let mut buffer = vec![0u8; MAX_RENDEZVOUS_PACKET];
    let result = loop {
        socket
            .send_to(&packet, rendezvous)
            .map_err(|_| Error::Transport("failed to register with temporary rendezvous"))?;
        match socket.recv_from(&mut buffer) {
            Ok((len, source)) if source == rendezvous => {
                let response: RendezvousServerPacket = serde_json::from_slice(&buffer[..len])
                    .map_err(|_| Error::Protocol("invalid rendezvous response"))?;
                match response {
                    RendezvousServerPacket::Peer {
                        room,
                        peer_addr,
                        peer,
                    } => {
                        if room != registration.room {
                            return Err(Error::Protocol("rendezvous room mismatch"));
                        }
                        let addr = peer_addr
                            .parse()
                            .map_err(|_| Error::Protocol("invalid rendezvous peer address"))?;
                        break Ok((addr, peer));
                    }
                    RendezvousServerPacket::Error { .. } => {
                        return Err(Error::Denied("temporary rendezvous rejected registration"));
                    }
                }
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Err(Error::Transport("temporary rendezvous receive failed")),
        }
        if Instant::now() >= deadline {
            break Err(Error::Timeout);
        }
    };
    let _ = socket.set_read_timeout(previous);
    result
}

pub(super) fn route_hello_transcript(
    room: &str,
    sender: &str,
    receiver: &str,
    nonce: &[u8; 32],
) -> Vec<u8> {
    transcript(
        b"aw-direct-route-hello-v1\0",
        &[
            room.as_bytes(),
            sender.as_bytes(),
            receiver.as_bytes(),
            nonce,
        ],
    )
}

pub(super) fn route_ack_transcript(
    room: &str,
    sender: &str,
    receiver: &str,
    request_nonce: &[u8; 32],
    nonce: &[u8; 32],
) -> Vec<u8> {
    transcript(
        b"aw-direct-route-ack-v1\0",
        &[
            room.as_bytes(),
            sender.as_bytes(),
            receiver.as_bytes(),
            request_nonce,
            nonce,
        ],
    )
}

pub(super) async fn direct_server_connection(
    endpoint: &Endpoint,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    room: &str,
    deadline: tokio::time::Instant,
) -> Result<quinn::Connection> {
    let incoming = tokio::time::timeout_at(deadline, endpoint.accept())
        .await
        .map_err(|_| Error::Timeout)?
        .ok_or(Error::Transport("direct QUIC endpoint closed"))?;
    let connection = incoming
        .await
        .map_err(|_| Error::Transport("direct QUIC handshake failed"))?;
    let (mut send, mut recv) = tokio::time::timeout_at(deadline, connection.accept_bi())
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Transport("direct QUIC route stream missing"))?;
    let hello: RouteHello = read_frame(&mut recv).await?;
    if hello.version != ROUTE_VERSION || hello.room != room || hello.node_id != peer.node_id {
        return Err(Error::Protocol("invalid direct route hello"));
    }
    let request_nonce = decode_fixed::<32>(&hello.nonce, "direct route nonce must be 32 bytes")?;
    let message = route_hello_transcript(room, &peer.node_id, &identity.node_id, &request_nonce);
    verify_signed_identity(&peer.node_id, &peer.public_key, &message, &hello.signature)?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for direct route ack"))?;
    let ack_message = route_ack_transcript(
        room,
        &identity.node_id,
        &peer.node_id,
        &request_nonce,
        &nonce,
    );
    let ack = RouteAck {
        version: ROUTE_VERSION,
        room: room.to_owned(),
        node_id: identity.node_id.clone(),
        request_nonce: hello.nonce,
        nonce: STANDARD.encode(nonce),
        signature: STANDARD.encode(identity.signing.sign(&ack_message).to_bytes()),
    };
    write_frame(&mut send, &ack).await?;
    Ok(connection)
}

pub(super) async fn direct_server_handshake(
    endpoint: &Endpoint,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    room: &str,
    deadline: tokio::time::Instant,
) -> Result<SocketAddr> {
    let connection = direct_server_connection(endpoint, identity, peer, room, deadline).await?;
    let remote = connection.remote_address();
    connection.close(0u32.into(), b"route verified");
    Ok(remote)
}

pub(super) async fn direct_client_connection(
    endpoint: &mut Endpoint,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    peer_cert: &[u8],
    peer_addr: SocketAddr,
    room: &str,
    deadline: tokio::time::Instant,
) -> Result<quinn::Connection> {
    endpoint.set_default_client_config(client_config(peer_cert)?);
    let connection = endpoint
        .connect(peer_addr, PAIR_SERVER_NAME)
        .map_err(|_| Error::Transport("cannot start direct QUIC route connection"))?;
    let connection = tokio::time::timeout_at(deadline, connection)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Transport("direct QUIC route connection failed"))?;
    let (mut send, mut recv) = tokio::time::timeout_at(deadline, connection.open_bi())
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Transport("cannot open direct QUIC route stream"))?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for direct route hello"))?;
    let message = route_hello_transcript(room, &identity.node_id, &peer.node_id, &nonce);
    let hello = RouteHello {
        version: ROUTE_VERSION,
        room: room.to_owned(),
        node_id: identity.node_id.clone(),
        nonce: STANDARD.encode(nonce),
        signature: STANDARD.encode(identity.signing.sign(&message).to_bytes()),
    };
    write_frame(&mut send, &hello).await?;
    let ack: RouteAck = tokio::time::timeout_at(deadline, read_frame(&mut recv))
        .await
        .map_err(|_| Error::Timeout)??;
    if ack.version != ROUTE_VERSION || ack.room != room || ack.node_id != peer.node_id {
        return Err(Error::Protocol("invalid direct route acknowledgement"));
    }
    let echoed = decode_fixed::<32>(
        &ack.request_nonce,
        "direct route echoed nonce must be 32 bytes",
    )?;
    if echoed != nonce {
        return Err(Error::Protocol(
            "direct route acknowledgement is not bound to this request",
        ));
    }
    let ack_nonce = decode_fixed::<32>(&ack.nonce, "direct route ack nonce must be 32 bytes")?;
    let ack_message =
        route_ack_transcript(room, &peer.node_id, &identity.node_id, &nonce, &ack_nonce);
    verify_signed_identity(
        &peer.node_id,
        &peer.public_key,
        &ack_message,
        &ack.signature,
    )?;
    Ok(connection)
}

pub(super) async fn direct_client_handshake(
    endpoint: &mut Endpoint,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    peer_cert: &[u8],
    peer_addr: SocketAddr,
    room: &str,
    deadline: tokio::time::Instant,
) -> Result<SocketAddr> {
    let connection = direct_client_connection(
        endpoint, identity, peer, peer_cert, peer_addr, room, deadline,
    )
    .await?;
    let remote = connection.remote_address();
    connection.close(0u32.into(), b"route verified");
    Ok(remote)
}

pub(super) struct PunchedQuic {
    pub(super) runtime: tokio::runtime::Runtime,
    pub(super) endpoint: Endpoint,
    pub(super) connection: quinn::Connection,
    pub(super) peer_node_id: String,
    pub(super) local_addr: SocketAddr,
    pub(super) peer_addr: SocketAddr,
    pub(super) role: &'static str,
    pub(super) discovered_candidates: Vec<RouteCandidate>,
}

pub(super) fn open_punched_quic(
    store: &SessionStore,
    state_dir: &Path,
    opts: PunchOptions,
) -> Result<PunchedQuic> {
    validate_room(&opts.room)?;
    if opts.timeout < Duration::from_secs(3) || opts.timeout > Duration::from_secs(120) {
        return Err(Error::Config(
            "direct reachability timeout must be between 3 and 120 seconds".into(),
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.peer_node_id)?;
    if peer.trust_state != "trusted" {
        return Err(Error::Denied(
            "direct route requires an already-trusted peer",
        ));
    }
    let identity = ensure_identity(store, state_dir)?;
    let socket = UdpSocket::bind(opts.listen)
        .map_err(|_| Error::Transport("cannot bind direct reachability UDP socket"))?;
    let mut report = gather_candidates(&socket, &opts.stun)?;
    let registration = build_registration(
        &identity,
        &opts.room,
        report.candidates.clone(),
        opts.timeout,
    )?;
    let (observed_peer, peer_registration) =
        rendezvous_exchange(&socket, opts.rendezvous, &registration, opts.timeout)?;
    validate_peer_registration(store, &opts.peer_node_id, &peer_registration)?;
    report.candidates.push(RouteCandidate {
        kind: CandidateKind::RendezvousObserved,
        address: observed_peer,
        priority: 3_000_000,
    });
    dedup_candidates(&mut report.candidates);
    for _ in 0..PUNCH_ROUNDS {
        let _ = socket.send_to(b"AWPUNCH1", observed_peer);
        thread::sleep(PUNCH_DELAY);
    }
    let peer_cert = STANDARD
        .decode(&peer_registration.cert_der)
        .map_err(|_| Error::Protocol("invalid peer route certificate"))?;
    let local_addr = socket.local_addr()?;
    let server_role = identity.node_id < peer.node_id;
    let runtime = runtime()?;
    let (endpoint, connection, peer_addr) = runtime.block_on(async {
        let quinn_runtime =
            quinn::default_runtime().ok_or(Error::Transport("no QUIC async runtime available"))?;
        let deadline = tokio::time::Instant::now() + opts.timeout;
        let mut endpoint = Endpoint::new(
            EndpointConfig::default(),
            if server_role {
                Some(server_config(&identity)?)
            } else {
                None
            },
            socket,
            Arc::clone(&quinn_runtime),
        )
        .map_err(|_| Error::Transport("cannot create direct QUIC endpoint from punched socket"))?;
        let connection = if server_role {
            direct_server_connection(&endpoint, &identity, &peer, &opts.room, deadline).await?
        } else {
            direct_client_connection(
                &mut endpoint,
                &identity,
                &peer,
                &peer_cert,
                observed_peer,
                &opts.room,
                deadline,
            )
            .await?
        };
        let peer_addr = connection.remote_address();
        Ok::<_, Error>((endpoint, connection, peer_addr))
    })?;
    Ok(PunchedQuic {
        runtime,
        endpoint,
        connection,
        peer_node_id: peer.node_id,
        local_addr,
        peer_addr,
        role: if server_role { "server" } else { "client" },
        discovered_candidates: report.candidates,
    })
}

pub fn punch_peer(
    store: &SessionStore,
    state_dir: &Path,
    opts: PunchOptions,
) -> Result<DirectRoute> {
    let link = open_punched_quic(store, state_dir, opts)?;
    let route = DirectRoute {
        peer_node_id: link.peer_node_id.clone(),
        local_addr: link.local_addr,
        peer_addr: link.peer_addr,
        transport: "quic_udp",
        candidate_kind: CandidateKind::RendezvousObserved,
        role: link.role,
        discovered_candidates: link.discovered_candidates.clone(),
    };
    link.connection
        .close(0u32.into(), b"reachability probe complete");
    link.endpoint
        .close(0u32.into(), b"reachability probe complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    Ok(route)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stun_xor_ipv4_decodes() {
        let tx = [3u8; 12];
        let mapped = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)), 43117);
        let mask = STUN_MAGIC.to_be_bytes();
        let ip = match mapped.ip() {
            IpAddr::V4(ip) => ip.octets(),
            _ => unreachable!(),
        };
        let mut response = vec![0u8; 32];
        response[0..2].copy_from_slice(&STUN_BINDING_SUCCESS.to_be_bytes());
        response[2..4].copy_from_slice(&12u16.to_be_bytes());
        response[4..8].copy_from_slice(&STUN_MAGIC.to_be_bytes());
        response[8..20].copy_from_slice(&tx);
        response[20..22].copy_from_slice(&STUN_XOR_MAPPED_ADDRESS.to_be_bytes());
        response[22..24].copy_from_slice(&8u16.to_be_bytes());
        response[24] = 0;
        response[25] = 1;
        response[26..28]
            .copy_from_slice(&(mapped.port() ^ ((STUN_MAGIC >> 16) as u16)).to_be_bytes());
        for i in 0..4 {
            response[28 + i] = ip[i] ^ mask[i];
        }
        assert_eq!(parse_stun_address(&response, &tx).unwrap(), mapped);
    }

    #[test]
    fn room_is_strict_canonical_hex() {
        assert!(validate_room(&"a".repeat(64)).is_ok());
        assert!(validate_room(&"A".repeat(64)).is_err());
        assert!(validate_room("short").is_err());
    }
}
