use super::reachability::{
    build_registration, direct_client_handshake, direct_server_handshake,
    validate_peer_registration, PunchOptions, RendezvousRegistration,
};
use super::*;
use quinn::EndpointConfig;
use rustls::{
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
    ClientConnection, RootCertStore as TlsRootCertStore, ServerConnection, StreamOwned,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const RELAY_VERSION: u16 = 1;
const MAX_RELAY_PACKET: usize = 16 * 1024;
const MAX_TCP_FRAME: usize = 32 * 1024;
const RELAY_RETRY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    DirectQuic,
    QuicRelay,
    TlsTcpRelay,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteAttempt {
    pub kind: RouteKind,
    pub outcome: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct EstablishedRoute {
    pub kind: RouteKind,
    pub peer_node_id: String,
    pub local_addr: SocketAddr,
    pub remote_addr: SocketAddr,
    pub role: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteSelection {
    pub selected: EstablishedRoute,
    pub attempts: Vec<RouteAttempt>,
}

#[derive(Clone, Debug)]
pub struct RelayServeOptions {
    pub listen: SocketAddr,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Serialize)]
pub struct RelayBound {
    pub transport: &'static str,
    pub address: SocketAddr,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RelayMatched {
    pub transport: &'static str,
    pub room: String,
    pub first_node: String,
    pub second_node: String,
}

#[derive(Clone, Debug)]
pub struct RelayServiceOptions {
    pub listen: SocketAddr,
    pub lifetime: Duration,
    pub room_ttl: Duration,
    pub max_rooms: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct RelayServiceStats {
    pub transport: &'static str,
    pub address: SocketAddr,
    pub rooms_created: u64,
    pub pairs_matched: u64,
    pub packets_forwarded: u64,
    pub expired_rooms: u64,
}

#[derive(Clone, Debug)]
pub struct RelayQuicOptions {
    pub peer_node_id: String,
    pub room: String,
    pub relay: SocketAddr,
    pub listen: SocketAddr,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct TlsTcpRelayOptions {
    pub peer_node_id: String,
    pub room: String,
    pub relay: SocketAddr,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct SelectRouteOptions {
    pub direct: Option<PunchOptions>,
    pub quic_relay: Option<RelayQuicOptions>,
    pub tcp_relay: Option<TlsTcpRelayOptions>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum UdpRelayClientPacket {
    Register {
        registration: RendezvousRegistration,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum UdpRelayServerPacket {
    Ready {
        room: String,
        peer: RendezvousRegistration,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TcpRelayRegister {
    version: u16,
    registration: RendezvousRegistration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TcpRelayReady {
    version: u16,
    room: String,
    peer: RendezvousRegistration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TlsHello {
    version: u16,
    room: String,
    node_id: String,
    nonce: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TlsAck {
    version: u16,
    room: String,
    node_id: String,
    request_nonce: String,
    nonce: String,
    signature: String,
}

fn validate_relay_timeout(timeout: Duration) -> Result<()> {
    if timeout < Duration::from_secs(3) || timeout > Duration::from_secs(300) {
        return Err(Error::Config(
            "relay timeout must be between 3 and 300 seconds".into(),
        ));
    }
    Ok(())
}

pub fn serve_udp_relay<F>(opts: RelayServeOptions, announce: F) -> Result<RelayMatched>
where
    F: FnOnce(&RelayBound) -> Result<()>,
{
    validate_relay_timeout(opts.timeout)?;
    runtime()?.block_on(async move {
        let socket = tokio::net::UdpSocket::bind(opts.listen)
            .await
            .map_err(|_| Error::Transport("cannot bind UDP relay"))?;
        let address = socket
            .local_addr()
            .map_err(|_| Error::Transport("cannot inspect UDP relay address"))?;
        announce(&RelayBound {
            transport: "quic_udp_relay",
            address,
            expires_at: now_secs().saturating_add(opts.timeout.as_secs()),
        })?;
        let deadline = tokio::time::Instant::now() + opts.timeout;
        let mut first: Option<(SocketAddr, RendezvousRegistration)> = None;
        let mut pair: Option<(
            SocketAddr,
            RendezvousRegistration,
            SocketAddr,
            RendezvousRegistration,
        )> = None;
        let mut buffer = vec![0u8; MAX_RELAY_PACKET];
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let wait = remaining.min(Duration::from_secs(1));
            let received = tokio::time::timeout(wait, socket.recv_from(&mut buffer)).await;
            let Ok(Ok((len, source))) = received else {
                if pair.is_some() {
                    break;
                }
                continue;
            };
            if let Some((a_addr, _, b_addr, _)) = &pair {
                let target = if source == *a_addr {
                    Some(*b_addr)
                } else if source == *b_addr {
                    Some(*a_addr)
                } else {
                    None
                };
                if let Some(target) = target {
                    socket
                        .send_to(&buffer[..len], target)
                        .await
                        .map_err(|_| Error::Transport("UDP relay forward failed"))?;
                }
                continue;
            }
            let packet: UdpRelayClientPacket = match serde_json::from_slice(&buffer[..len]) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let UdpRelayClientPacket::Register { registration } = packet;
            super::reachability::validate_registration_shape(&registration)?;
            if let Some((first_addr, first_registration)) = first.clone() {
                if first_registration.room != registration.room
                    || first_registration.node_id == registration.node_id
                {
                    continue;
                }
                let a = UdpRelayServerPacket::Ready {
                    room: registration.room.clone(),
                    peer: registration.clone(),
                };
                let b = UdpRelayServerPacket::Ready {
                    room: registration.room.clone(),
                    peer: first_registration.clone(),
                };
                let a_bytes = serde_json::to_vec(&a)
                    .map_err(|_| Error::Protocol("cannot encode UDP relay ready"))?;
                let b_bytes = serde_json::to_vec(&b)
                    .map_err(|_| Error::Protocol("cannot encode UDP relay ready"))?;
                socket
                    .send_to(&a_bytes, first_addr)
                    .await
                    .map_err(|_| Error::Transport("UDP relay ready send failed"))?;
                socket
                    .send_to(&b_bytes, source)
                    .await
                    .map_err(|_| Error::Transport("UDP relay ready send failed"))?;
                pair = Some((first_addr, first_registration, source, registration));
            } else {
                first = Some((source, registration));
            }
        }
        let Some((_, a, _, b)) = pair else {
            return Err(Error::Timeout);
        };
        Ok(RelayMatched {
            transport: "quic_udp_relay",
            room: a.room,
            first_node: a.node_id,
            second_node: b.node_id,
        })
    })
}

enum RelayRoom {
    Waiting {
        address: SocketAddr,
        registration: RendezvousRegistration,
        expires: tokio::time::Instant,
    },
    Paired {
        first_address: SocketAddr,
        second_address: SocketAddr,
        expires: tokio::time::Instant,
    },
}

pub fn serve_udp_relay_service<F>(
    opts: RelayServiceOptions,
    announce: F,
) -> Result<RelayServiceStats>
where
    F: FnOnce(&RelayBound) -> Result<()>,
{
    validate_relay_timeout(opts.lifetime)?;
    if opts.room_ttl < Duration::from_secs(3)
        || opts.room_ttl > Duration::from_secs(3600)
        || opts.max_rooms == 0
        || opts.max_rooms > 4096
    {
        return Err(Error::Config(
            "relay service requires room TTL 3..3600 seconds and max_rooms 1..4096".into(),
        ));
    }
    runtime()?.block_on(async move {
        let socket = tokio::net::UdpSocket::bind(opts.listen)
            .await
            .map_err(|_| Error::Transport("cannot bind reusable UDP relay"))?;
        let address = socket
            .local_addr()
            .map_err(|_| Error::Transport("cannot inspect reusable UDP relay address"))?;
        announce(&RelayBound {
            transport: "awnet_udp_relay_service",
            address,
            expires_at: now_secs().saturating_add(opts.lifetime.as_secs()),
        })?;
        let deadline = tokio::time::Instant::now() + opts.lifetime;
        let mut rooms: HashMap<String, RelayRoom> = HashMap::new();
        let mut routes: HashMap<SocketAddr, (SocketAddr, String)> = HashMap::new();
        let mut stats = RelayServiceStats {
            transport: "awnet_udp_relay_service",
            address,
            rooms_created: 0,
            pairs_matched: 0,
            packets_forwarded: 0,
            expired_rooms: 0,
        };
        let mut buffer = vec![0u8; MAX_RELAY_PACKET];
        while tokio::time::Instant::now() < deadline {
            let now = tokio::time::Instant::now();
            let expired = rooms
                .iter()
                .filter_map(|(room, state)| {
                    let expires = match state {
                        RelayRoom::Waiting { expires, .. } | RelayRoom::Paired { expires, .. } => {
                            *expires
                        }
                    };
                    (expires <= now).then_some(room.clone())
                })
                .collect::<Vec<_>>();
            for room in expired {
                if let Some(RelayRoom::Paired {
                    first_address,
                    second_address,
                    ..
                }) = rooms.remove(&room)
                {
                    routes.remove(&first_address);
                    routes.remove(&second_address);
                } else {
                    rooms.remove(&room);
                }
                stats.expired_rooms = stats.expired_rooms.saturating_add(1);
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let wait = remaining.min(Duration::from_secs(1));
            let received = tokio::time::timeout(wait, socket.recv_from(&mut buffer)).await;
            let Ok(Ok((len, source))) = received else {
                continue;
            };
            if let Some((target, _room)) = routes.get(&source) {
                socket
                    .send_to(&buffer[..len], *target)
                    .await
                    .map_err(|_| Error::Transport("reusable UDP relay forward failed"))?;
                stats.packets_forwarded = stats.packets_forwarded.saturating_add(1);
                continue;
            }
            let packet: UdpRelayClientPacket = match serde_json::from_slice(&buffer[..len]) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let UdpRelayClientPacket::Register { registration } = packet;
            super::reachability::validate_registration_shape(&registration)?;
            let room_name = registration.room.clone();
            match rooms.remove(&room_name) {
                Some(RelayRoom::Waiting {
                    address: first_address,
                    registration: first_registration,
                    ..
                }) if first_registration.node_id != registration.node_id => {
                    let first_ready = UdpRelayServerPacket::Ready {
                        room: room_name.clone(),
                        peer: registration.clone(),
                    };
                    let second_ready = UdpRelayServerPacket::Ready {
                        room: room_name.clone(),
                        peer: first_registration,
                    };
                    let first_bytes = serde_json::to_vec(&first_ready)
                        .map_err(|_| Error::Protocol("cannot encode relay service ready"))?;
                    let second_bytes = serde_json::to_vec(&second_ready)
                        .map_err(|_| Error::Protocol("cannot encode relay service ready"))?;
                    socket
                        .send_to(&first_bytes, first_address)
                        .await
                        .map_err(|_| Error::Transport("relay service ready send failed"))?;
                    socket
                        .send_to(&second_bytes, source)
                        .await
                        .map_err(|_| Error::Transport("relay service ready send failed"))?;
                    routes.insert(first_address, (source, room_name.clone()));
                    routes.insert(source, (first_address, room_name.clone()));
                    rooms.insert(
                        room_name,
                        RelayRoom::Paired {
                            first_address,
                            second_address: source,
                            expires: tokio::time::Instant::now() + opts.room_ttl,
                        },
                    );
                    stats.pairs_matched = stats.pairs_matched.saturating_add(1);
                }
                Some(existing) => {
                    rooms.insert(room_name, existing);
                }
                None if rooms.len() < opts.max_rooms => {
                    rooms.insert(
                        room_name,
                        RelayRoom::Waiting {
                            address: source,
                            registration,
                            expires: tokio::time::Instant::now() + opts.room_ttl,
                        },
                    );
                    stats.rooms_created = stats.rooms_created.saturating_add(1);
                }
                None => {
                    // Admission is bounded: silently ignore new rooms once full.
                }
            }
        }
        Ok(stats)
    })
}

fn udp_relay_exchange(
    socket: &UdpSocket,
    relay: SocketAddr,
    registration: &RendezvousRegistration,
    timeout: Duration,
) -> Result<RendezvousRegistration> {
    let packet = serde_json::to_vec(&UdpRelayClientPacket::Register {
        registration: registration.clone(),
    })
    .map_err(|_| Error::Protocol("cannot encode UDP relay registration"))?;
    if packet.len() > MAX_RELAY_PACKET {
        return Err(Error::Limit("UDP relay registration exceeds packet bound"));
    }
    let previous = socket.read_timeout()?;
    socket.set_read_timeout(Some(RELAY_RETRY))?;
    let deadline = Instant::now() + timeout;
    let mut buffer = vec![0u8; MAX_RELAY_PACKET];
    let result = loop {
        socket
            .send_to(&packet, relay)
            .map_err(|_| Error::Transport("failed to register with UDP relay"))?;
        match socket.recv_from(&mut buffer) {
            Ok((len, source)) if source == relay => {
                let response: UdpRelayServerPacket = serde_json::from_slice(&buffer[..len])
                    .map_err(|_| Error::Protocol("invalid UDP relay response"))?;
                let UdpRelayServerPacket::Ready { room, peer } = response;
                if room != registration.room {
                    return Err(Error::Protocol("UDP relay room mismatch"));
                }
                break Ok(peer);
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Err(Error::Transport("UDP relay receive failed")),
        }
        if Instant::now() >= deadline {
            break Err(Error::Timeout);
        }
    };
    let _ = socket.set_read_timeout(previous);
    result
}

pub fn relay_quic_peer(
    store: &SessionStore,
    state_dir: &Path,
    opts: RelayQuicOptions,
) -> Result<EstablishedRoute> {
    validate_relay_timeout(opts.timeout)?;
    super::reachability::validate_room(&opts.room)?;
    let peer = store.awnet_peer_by_node_id(&opts.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let socket = UdpSocket::bind(opts.listen)
        .map_err(|_| Error::Transport("cannot bind UDP relay route socket"))?;
    let candidates = gather_candidates(&socket, &[])?.candidates;
    let registration = build_registration(&identity, &opts.room, candidates, opts.timeout)?;
    let peer_registration = udp_relay_exchange(&socket, opts.relay, &registration, opts.timeout)?;
    validate_peer_registration(store, &opts.peer_node_id, &peer_registration)?;
    let peer_cert = STANDARD
        .decode(&peer_registration.cert_der)
        .map_err(|_| Error::Protocol("invalid peer relay certificate"))?;
    let local_addr = socket.local_addr()?;
    let server_role = identity.node_id < peer.node_id;
    runtime()?.block_on(async move {
        let runtime =
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
            Arc::clone(&runtime),
        )
        .map_err(|_| Error::Transport("cannot create QUIC endpoint for UDP relay"))?;
        let remote_addr = if server_role {
            direct_server_handshake(&endpoint, &identity, &peer, &opts.room, deadline).await?
        } else {
            direct_client_handshake(
                &mut endpoint,
                &identity,
                &peer,
                &peer_cert,
                opts.relay,
                &opts.room,
                deadline,
            )
            .await?
        };
        endpoint.close(0u32.into(), b"relay route verified");
        endpoint.wait_idle().await;
        Ok(EstablishedRoute {
            kind: RouteKind::QuicRelay,
            peer_node_id: peer.node_id,
            local_addr,
            remote_addr,
            role: if server_role { "server" } else { "client" },
        })
    })
}

fn write_sync_frame<T: Serialize, W: std::io::Write>(writer: &mut W, value: &T) -> Result<()> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| Error::Protocol("cannot encode TCP relay frame"))?;
    if bytes.is_empty() || bytes.len() > MAX_TCP_FRAME {
        return Err(Error::Limit("TCP relay frame exceeds bounded size"));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

fn read_sync_frame<T: DeserializeOwned, R: std::io::Read>(reader: &mut R) -> Result<T> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_TCP_FRAME {
        return Err(Error::Limit("TCP relay frame exceeds bounded size"));
    }
    let mut bytes = vec![0u8; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol("invalid TCP relay frame"))
}

fn read_tcp_registration(stream: &mut TcpStream) -> Result<RendezvousRegistration> {
    let register: TcpRelayRegister = read_sync_frame(stream)?;
    if register.version != RELAY_VERSION {
        return Err(Error::Protocol("unsupported TCP relay registration"));
    }
    super::reachability::validate_registration_shape(&register.registration)?;
    Ok(register.registration)
}

pub fn serve_tcp_relay<F>(opts: RelayServeOptions, announce: F) -> Result<RelayMatched>
where
    F: FnOnce(&RelayBound) -> Result<()>,
{
    validate_relay_timeout(opts.timeout)?;
    let listener = TcpListener::bind(opts.listen)
        .map_err(|_| Error::Transport("cannot bind TLS/TCP relay"))?;
    let address = listener.local_addr()?;
    announce(&RelayBound {
        transport: "tls_tcp_relay",
        address,
        expires_at: now_secs().saturating_add(opts.timeout.as_secs()),
    })?;
    let (mut first, _) = listener
        .accept()
        .map_err(|_| Error::Transport("TCP relay first accept failed"))?;
    first.set_read_timeout(Some(opts.timeout))?;
    first.set_write_timeout(Some(opts.timeout))?;
    let first_registration = read_tcp_registration(&mut first)?;
    let (mut second, _) = listener
        .accept()
        .map_err(|_| Error::Transport("TCP relay second accept failed"))?;
    second.set_read_timeout(Some(opts.timeout))?;
    second.set_write_timeout(Some(opts.timeout))?;
    let second_registration = read_tcp_registration(&mut second)?;
    if first_registration.room != second_registration.room
        || first_registration.node_id == second_registration.node_id
    {
        return Err(Error::Denied(
            "TCP relay registrations do not form a peer pair",
        ));
    }
    write_sync_frame(
        &mut first,
        &TcpRelayReady {
            version: RELAY_VERSION,
            room: first_registration.room.clone(),
            peer: second_registration.clone(),
        },
    )?;
    write_sync_frame(
        &mut second,
        &TcpRelayReady {
            version: RELAY_VERSION,
            room: first_registration.room.clone(),
            peer: first_registration.clone(),
        },
    )?;
    let mut first_read = first.try_clone()?;
    let mut second_write = second.try_clone()?;
    let left = thread::spawn(move || std::io::copy(&mut first_read, &mut second_write));
    let mut second_read = second;
    let mut first_write = first;
    let right = thread::spawn(move || std::io::copy(&mut second_read, &mut first_write));
    let _ = left.join();
    let _ = right.join();
    Ok(RelayMatched {
        transport: "tls_tcp_relay",
        room: first_registration.room,
        first_node: first_registration.node_id,
        second_node: second_registration.node_id,
    })
}

fn tcp_relay_exchange(
    relay: SocketAddr,
    registration: &RendezvousRegistration,
    timeout: Duration,
) -> Result<(TcpStream, RendezvousRegistration)> {
    let mut stream = TcpStream::connect_timeout(&relay, timeout)
        .map_err(|_| Error::Transport("cannot connect to TLS/TCP relay"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write_sync_frame(
        &mut stream,
        &TcpRelayRegister {
            version: RELAY_VERSION,
            registration: registration.clone(),
        },
    )?;
    let ready: TcpRelayReady = read_sync_frame(&mut stream)?;
    if ready.version != RELAY_VERSION || ready.room != registration.room {
        return Err(Error::Protocol("invalid TLS/TCP relay ready frame"));
    }
    Ok((stream, ready.peer))
}

fn rustls_server(identity: &NodeIdentity) -> Result<rustls::ServerConfig> {
    // Match AwNet's QUIC provider without relying on process-wide feature inference.
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|_| Error::Transport("cannot configure TLS fallback server protocol versions"))?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(identity.quic_cert_der.clone())],
            PrivatePkcs8KeyDer::from(identity.quic_key_der.clone()).into(),
        )
        .map_err(|_| Error::Storage("invalid stored TLS fallback certificate/key"))
}

fn rustls_client(peer_cert: &[u8]) -> Result<rustls::ClientConfig> {
    let mut roots = TlsRootCertStore::empty();
    roots
        .add(CertificateDer::from(peer_cert.to_vec()))
        .map_err(|_| Error::Protocol("invalid peer TLS fallback certificate"))?;
    Ok(rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| Error::Transport("cannot configure TLS fallback client protocol versions"))?
    .with_root_certificates(roots)
    .with_no_client_auth())
}

fn tls_server_handshake(
    stream: TcpStream,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    room: &str,
) -> Result<SocketAddr> {
    let remote = stream.peer_addr()?;
    let connection = ServerConnection::new(Arc::new(rustls_server(identity)?))
        .map_err(|_| Error::Transport("cannot create TLS fallback server"))?;
    let mut tls = StreamOwned::new(connection, stream);
    let hello: TlsHello = read_sync_frame(&mut tls)?;
    if hello.version != RELAY_VERSION || hello.room != room || hello.node_id != peer.node_id {
        return Err(Error::Protocol("invalid TLS fallback hello"));
    }
    let request_nonce = decode_fixed::<32>(&hello.nonce, "TLS fallback nonce must be 32 bytes")?;
    let message = super::reachability::route_hello_transcript(
        room,
        &peer.node_id,
        &identity.node_id,
        &request_nonce,
    );
    verify_signed_identity(&peer.node_id, &peer.public_key, &message, &hello.signature)?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for TLS fallback ack"))?;
    let ack_message = super::reachability::route_ack_transcript(
        room,
        &identity.node_id,
        &peer.node_id,
        &request_nonce,
        &nonce,
    );
    write_sync_frame(
        &mut tls,
        &TlsAck {
            version: RELAY_VERSION,
            room: room.to_owned(),
            node_id: identity.node_id.clone(),
            request_nonce: hello.nonce,
            nonce: STANDARD.encode(nonce),
            signature: STANDARD.encode(identity.signing.sign(&ack_message).to_bytes()),
        },
    )?;
    Ok(remote)
}

fn tls_client_handshake(
    stream: TcpStream,
    identity: &NodeIdentity,
    peer: &AwNetPeerInfo,
    peer_cert: &[u8],
    room: &str,
) -> Result<SocketAddr> {
    let remote = stream.peer_addr()?;
    let server_name = ServerName::try_from(PAIR_SERVER_NAME)
        .map_err(|_| Error::Protocol("invalid TLS fallback server name"))?;
    let connection = ClientConnection::new(Arc::new(rustls_client(peer_cert)?), server_name)
        .map_err(|_| Error::Transport("cannot create TLS fallback client"))?;
    let mut tls = StreamOwned::new(connection, stream);
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for TLS fallback hello"))?;
    let message =
        super::reachability::route_hello_transcript(room, &identity.node_id, &peer.node_id, &nonce);
    write_sync_frame(
        &mut tls,
        &TlsHello {
            version: RELAY_VERSION,
            room: room.to_owned(),
            node_id: identity.node_id.clone(),
            nonce: STANDARD.encode(nonce),
            signature: STANDARD.encode(identity.signing.sign(&message).to_bytes()),
        },
    )?;
    let ack: TlsAck = read_sync_frame(&mut tls)?;
    if ack.version != RELAY_VERSION || ack.room != room || ack.node_id != peer.node_id {
        return Err(Error::Protocol("invalid TLS fallback acknowledgement"));
    }
    let echoed = decode_fixed::<32>(
        &ack.request_nonce,
        "TLS fallback echoed nonce must be 32 bytes",
    )?;
    if echoed != nonce {
        return Err(Error::Protocol(
            "TLS fallback acknowledgement is not bound to this request",
        ));
    }
    let ack_nonce = decode_fixed::<32>(&ack.nonce, "TLS fallback ack nonce must be 32 bytes")?;
    let ack_message = super::reachability::route_ack_transcript(
        room,
        &peer.node_id,
        &identity.node_id,
        &nonce,
        &ack_nonce,
    );
    verify_signed_identity(
        &peer.node_id,
        &peer.public_key,
        &ack_message,
        &ack.signature,
    )?;
    Ok(remote)
}

pub fn relay_tcp_peer(
    store: &SessionStore,
    state_dir: &Path,
    opts: TlsTcpRelayOptions,
) -> Result<EstablishedRoute> {
    validate_relay_timeout(opts.timeout)?;
    super::reachability::validate_room(&opts.room)?;
    let peer = store.awnet_peer_by_node_id(&opts.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let registration = build_registration(&identity, &opts.room, Vec::new(), opts.timeout)?;
    let (stream, peer_registration) = tcp_relay_exchange(opts.relay, &registration, opts.timeout)?;
    validate_peer_registration(store, &opts.peer_node_id, &peer_registration)?;
    let peer_cert = STANDARD
        .decode(&peer_registration.cert_der)
        .map_err(|_| Error::Protocol("invalid peer TLS fallback certificate"))?;
    let local_addr = stream.local_addr()?;
    let server_role = identity.node_id < peer.node_id;
    let remote_addr = if server_role {
        tls_server_handshake(stream, &identity, &peer, &opts.room)?
    } else {
        tls_client_handshake(stream, &identity, &peer, &peer_cert, &opts.room)?
    };
    Ok(EstablishedRoute {
        kind: RouteKind::TlsTcpRelay,
        peer_node_id: peer.node_id,
        local_addr,
        remote_addr,
        role: if server_role { "server" } else { "client" },
    })
}

pub fn select_route(
    store: &SessionStore,
    state_dir: &Path,
    opts: SelectRouteOptions,
) -> Result<RouteSelection> {
    let mut attempts = Vec::new();
    if let Some(direct) = opts.direct {
        match punch_peer(store, state_dir, direct) {
            Ok(route) => {
                attempts.push(RouteAttempt {
                    kind: RouteKind::DirectQuic,
                    outcome: "selected",
                });
                return Ok(RouteSelection {
                    selected: EstablishedRoute {
                        kind: RouteKind::DirectQuic,
                        peer_node_id: route.peer_node_id,
                        local_addr: route.local_addr,
                        remote_addr: route.peer_addr,
                        role: route.role,
                    },
                    attempts,
                });
            }
            Err(_) => attempts.push(RouteAttempt {
                kind: RouteKind::DirectQuic,
                outcome: "unavailable",
            }),
        }
    }
    if let Some(relay) = opts.quic_relay {
        match relay_quic_peer(store, state_dir, relay) {
            Ok(route) => {
                attempts.push(RouteAttempt {
                    kind: RouteKind::QuicRelay,
                    outcome: "selected",
                });
                return Ok(RouteSelection {
                    selected: route,
                    attempts,
                });
            }
            Err(_) => attempts.push(RouteAttempt {
                kind: RouteKind::QuicRelay,
                outcome: "unavailable",
            }),
        }
    }
    if let Some(relay) = opts.tcp_relay {
        match relay_tcp_peer(store, state_dir, relay) {
            Ok(route) => {
                attempts.push(RouteAttempt {
                    kind: RouteKind::TlsTcpRelay,
                    outcome: "selected",
                });
                return Ok(RouteSelection {
                    selected: route,
                    attempts,
                });
            }
            Err(_) => attempts.push(RouteAttempt {
                kind: RouteKind::TlsTcpRelay,
                outcome: "unavailable",
            }),
        }
    }
    Err(Error::Transport(
        "no authenticated AwNet route succeeded; durable peer trust is unchanged",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_kind_serialization_is_stable() {
        assert_eq!(
            serde_json::to_string(&RouteKind::TlsTcpRelay).unwrap(),
            "\"tls_tcp_relay\""
        );
    }

    #[test]
    fn relay_timeout_is_bounded() {
        assert!(validate_relay_timeout(Duration::from_secs(2)).is_err());
        assert!(validate_relay_timeout(Duration::from_secs(30)).is_ok());
        assert!(validate_relay_timeout(Duration::from_secs(301)).is_err());
    }

    #[test]
    fn reusable_relay_bounds_rooms_before_binding() {
        let result = serve_udp_relay_service(
            RelayServiceOptions {
                listen: "127.0.0.1:0".parse().unwrap(),
                lifetime: Duration::from_secs(30),
                room_ttl: Duration::from_secs(120),
                max_rooms: 0,
            },
            |_| Ok(()),
        );
        assert!(result.is_err());
    }
}
