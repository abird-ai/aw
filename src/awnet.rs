//! First AwNet transport slice: stable AwNode identity plus one-use direct QUIC pairing.
//!
//! This module deliberately does not expose Workspace, Session, file, Git, PTY or
//! agent RPCs. Pairing establishes peer identity only. Domain authority remains in
//! the durable control plane and remote execution continues to fail closed.

use std::{
    fs::File,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aw::{Error, Result};
use aw_store::{AwNetPeerInfo, SessionStore};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use quinn::{ClientConfig, Endpoint, ServerConfig};
use rustls::{
    pki_types::{CertificateDer, PrivatePkcs8KeyDer},
    RootCertStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::path::PathBuf;
use subtle::ConstantTimeEq;

mod authority;
mod authority_replica;
#[cfg(target_os = "linux")]
mod actor_remote;
mod federation;
mod fleet_dispatch;
mod identity_continuity;
mod machinelink;
mod reachability;
#[cfg(target_os = "linux")]
mod remote_control;
mod remote_sessions;
mod routes;
mod services;
mod workspace_replica;
pub use authority::*;
pub use authority_replica::*;
#[cfg(target_os = "linux")]
pub use actor_remote::*;
pub use federation::*;
pub use fleet_dispatch::*;
pub use identity_continuity::*;
pub use machinelink::*;
pub use reachability::*;
#[cfg(target_os = "linux")]
pub use remote_control::*;
pub use remote_sessions::*;
pub use routes::*;
pub use services::*;
pub use workspace_replica::*;

const IDENTITY_FILE: &str = "node-identity.json";
const IDENTITY_VERSION: u16 = 1;
const PAIR_VERSION: u16 = 1;
const PAIR_PREFIX: &str = "awpair1_";
const PAIR_SERVER_NAME: &str = "aw-pair.local";
const MAX_IDENTITY_BYTES: usize = 64 * 1024;
const MAX_PAIR_CODE_BYTES: usize = 16 * 1024;
const MAX_FRAME_BYTES: usize = 16 * 1024;
const MAX_PAIR_ATTEMPTS: usize = 32;
const FRAME_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityFile {
    version: u16,
    seed: String,
    quic_key_der: String,
    quic_cert_der: String,
}

struct NodeIdentity {
    signing: SigningKey,
    node_id: String,
    key_fingerprint: String,
    public_key: String,
    quic_key_der: Vec<u8>,
    quic_cert_der: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairingCode {
    version: u16,
    node_id: String,
    public_key: String,
    display_name: String,
    endpoint: String,
    token: String,
    expires_at: u64,
    quic_cert_der: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairRequest {
    version: u16,
    inviter_node_id: String,
    node_id: String,
    public_key: String,
    display_name: String,
    token: String,
    nonce: String,
    signature: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairResponse {
    version: u16,
    accepted: bool,
    node_id: Option<String>,
    public_key: Option<String>,
    display_name: Option<String>,
    request_nonce: Option<String>,
    nonce: Option<String>,
    signature: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct PairOffer {
    pub code: String,
    pub node_id: String,
    pub endpoint: SocketAddr,
    pub expires_at: u64,
}

pub struct OfferOptions {
    pub listen: SocketAddr,
    pub advertise: Option<SocketAddr>,
    pub display_name: String,
    pub timeout: Duration,
}

struct PairListener {
    endpoint: Endpoint,
    identity: NodeIdentity,
    code: PairingCode,
    deadline: tokio::time::Instant,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}

fn node_id_from_public_key(public_key: &[u8; 32]) -> String {
    format!("node_{}", hex(&Sha256::digest(public_key)))
}

fn fingerprint_from_public_key(public_key: &[u8; 32]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(public_key)))
}

fn validate_display_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(Error::Config("invalid bounded AwNode display name".into()));
    }
    Ok(())
}

fn decode_fixed<const N: usize>(value: &str, what: &'static str) -> Result<[u8; N]> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| Error::Protocol("invalid base64 in AwNet pairing frame"))?;
    bytes.try_into().map_err(|_| Error::Protocol(what))
}

fn decode_hex_fixed<const N: usize>(value: &str, what: &'static str) -> Result<[u8; N]> {
    if value.len() != N * 2 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Protocol(what));
    }
    let mut out = [0u8; N];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let pair = std::str::from_utf8(pair).map_err(|_| Error::Protocol(what))?;
        out[index] = u8::from_str_radix(pair, 16).map_err(|_| Error::Protocol(what))?;
    }
    Ok(out)
}

fn transcript(label: &[u8], fields: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(label);
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

fn request_transcript(
    token: &[u8; 32],
    inviter: &str,
    requester: &str,
    nonce: &[u8; 32],
) -> Vec<u8> {
    transcript(
        b"aw-pair-request-v1\0",
        &[token, inviter.as_bytes(), requester.as_bytes(), nonce],
    )
}

fn response_transcript(
    token: &[u8; 32],
    inviter: &str,
    requester: &str,
    request_nonce: &[u8; 32],
    response_nonce: &[u8; 32],
) -> Vec<u8> {
    transcript(
        b"aw-pair-response-v1\0",
        &[
            token,
            inviter.as_bytes(),
            requester.as_bytes(),
            request_nonce,
            response_nonce,
        ],
    )
}

fn verify_signed_identity(
    node_id: &str,
    public_key: &str,
    message: &[u8],
    signature: &str,
) -> Result<()> {
    let public = decode_hex_fixed::<32>(
        public_key,
        "AwNode public key must be 32-byte lowercase/uppercase hex",
    )?;
    if node_id_from_public_key(&public) != node_id {
        return Err(Error::Protocol("AwNode ID does not match its public key"));
    }
    let verifying = VerifyingKey::from_bytes(&public)
        .map_err(|_| Error::Protocol("invalid Ed25519 AwNode public key"))?;
    let signature_bytes = decode_fixed::<64>(signature, "AwNode signature must be 64 bytes")?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying
        .verify_strict(message, &signature)
        .map_err(|_| Error::Protocol("AwNode pairing signature verification failed"))
}

fn open_private_existing(path: &Path) -> Result<File> {
    aw_platform::open_private_existing(path, false).map_err(|_| {
        Error::Denied("AwNode identity requires platform-verified owner-only private custody")
    })
}

fn create_private_new(path: &Path) -> Result<File> {
    aw_platform::open_private_new(path).map_err(|_| {
        Error::Denied("AwNode identity creation requires platform-verified owner-only private custody")
    })
}

fn read_identity_file(path: &Path) -> Result<IdentityFile> {
    let mut file = open_private_existing(path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_IDENTITY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_IDENTITY_BYTES {
        return Err(Error::Limit(
            "AwNode identity file exceeds the bounded format limit",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Storage("invalid AwNode identity file"))
}

fn generate_identity_file() -> Result<IdentityFile> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed)
        .map_err(|_| Error::Storage("OS randomness unavailable for AwNode identity"))?;
    let certified = rcgen::generate_simple_self_signed(vec![PAIR_SERVER_NAME.to_owned()])
        .map_err(|_| Error::Storage("cannot generate QUIC pairing certificate"))?;
    Ok(IdentityFile {
        version: IDENTITY_VERSION,
        seed: STANDARD.encode(seed),
        quic_key_der: STANDARD.encode(certified.signing_key.serialize_der()),
        quic_cert_der: STANDARD.encode(certified.cert.der()),
    })
}

fn write_identity_file(path: &Path, identity: &IdentityFile) -> Result<()> {
    let bytes = serde_json::to_vec(identity)
        .map_err(|_| Error::Storage("cannot encode AwNode identity"))?;
    if bytes.len() > MAX_IDENTITY_BYTES {
        return Err(Error::Limit(
            "generated AwNode identity exceeds the bounded format limit",
        ));
    }
    let mut file = create_private_new(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    aw_platform::sync_parent(path)?;
    Ok(())
}

fn materialize_identity(raw: IdentityFile) -> Result<NodeIdentity> {
    if raw.version != IDENTITY_VERSION {
        return Err(Error::Storage("unsupported AwNode identity file version"));
    }
    let seed = decode_fixed::<32>(&raw.seed, "AwNode secret seed must be 32 bytes")?;
    let signing = SigningKey::from_bytes(&seed);
    let public = signing.verifying_key().to_bytes();
    let quic_key_der = STANDARD
        .decode(raw.quic_key_der)
        .map_err(|_| Error::Storage("invalid QUIC key in AwNode identity file"))?;
    let quic_cert_der = STANDARD
        .decode(raw.quic_cert_der)
        .map_err(|_| Error::Storage("invalid QUIC certificate in AwNode identity file"))?;
    if quic_key_der.is_empty()
        || quic_key_der.len() > 16 * 1024
        || quic_cert_der.is_empty()
        || quic_cert_der.len() > 16 * 1024
    {
        return Err(Error::Storage(
            "invalid bounded QUIC material in AwNode identity file",
        ));
    }
    Ok(NodeIdentity {
        signing,
        node_id: node_id_from_public_key(&public),
        key_fingerprint: fingerprint_from_public_key(&public),
        public_key: hex(&public),
        quic_key_der,
        quic_cert_der,
    })
}

fn ensure_identity(store: &SessionStore, state_dir: &Path) -> Result<NodeIdentity> {
    let path = state_dir.join(IDENTITY_FILE);
    let raw = if path.exists() {
        read_identity_file(&path)?
    } else {
        if store.local_machine()?.key_fingerprint.is_some() {
            return Err(Error::Denied("persisted AwNode identity exists but node-identity.json is missing; restore it instead of rotating identity implicitly"));
        }
        let generated = generate_identity_file()?;
        write_identity_file(&path, &generated)?;
        generated
    };
    let identity = materialize_identity(raw)?;
    let _ = server_config(&identity)?;
    store.bind_local_node_identity(&identity.node_id, &identity.key_fingerprint)?;
    Ok(identity)
}

fn server_config(identity: &NodeIdentity) -> Result<ServerConfig> {
    let cert = CertificateDer::from(identity.quic_cert_der.clone());
    let key = PrivatePkcs8KeyDer::from(identity.quic_key_der.clone());
    ServerConfig::with_single_cert(vec![cert], key.into())
        .map_err(|_| Error::Storage("invalid stored QUIC server certificate/key"))
}

fn client_config(cert_der: &[u8]) -> Result<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(cert_der.to_vec()))
        .map_err(|_| Error::Protocol("pairing code contains an invalid QUIC certificate"))?;
    ClientConfig::with_root_certificates(Arc::new(roots))
        .map_err(|_| Error::Transport("cannot construct certificate-pinned QUIC client"))
}

fn discover_advertise(bound: SocketAddr) -> SocketAddr {
    if !bound.ip().is_unspecified() {
        return bound;
    }
    let probe = match bound.ip() {
        IpAddr::V4(_) => ("0.0.0.0:0", "1.1.1.1:80"),
        IpAddr::V6(_) => ("[::]:0", "[2606:4700:4700::1111]:80"),
    };
    if let Ok(socket) = UdpSocket::bind(probe.0) {
        if socket.connect(probe.1).is_ok() {
            if let Ok(local) = socket.local_addr() {
                return SocketAddr::new(local.ip(), bound.port());
            }
        }
    }
    match bound.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bound.port()),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), bound.port()),
    }
}

fn encode_pairing_code(code: &PairingCode) -> Result<String> {
    let raw = serde_json::to_vec(code).map_err(|_| Error::Storage("cannot encode pairing code"))?;
    if raw.len() > MAX_PAIR_CODE_BYTES {
        return Err(Error::Limit("pairing code exceeds bounded size"));
    }
    Ok(format!("{PAIR_PREFIX}{}", URL_SAFE_NO_PAD.encode(raw)))
}

fn decode_pairing_code(value: &str) -> Result<PairingCode> {
    let encoded = value
        .strip_prefix(PAIR_PREFIX)
        .ok_or_else(|| Error::Config("pairing code must start with awpair1_".into()))?;
    if encoded.len() > MAX_PAIR_CODE_BYTES * 2 {
        return Err(Error::Limit("pairing code exceeds bounded size"));
    }
    let raw = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Error::Config("invalid AwNet pairing code".into()))?;
    if raw.len() > MAX_PAIR_CODE_BYTES {
        return Err(Error::Limit("pairing code exceeds bounded size"));
    }
    let code: PairingCode = serde_json::from_slice(&raw)
        .map_err(|_| Error::Config("invalid AwNet pairing code".into()))?;
    validate_pairing_code(&code)?;
    Ok(code)
}

fn validate_pairing_code(code: &PairingCode) -> Result<()> {
    if code.version != PAIR_VERSION {
        return Err(Error::Config(
            "unsupported AwNet pairing code version".into(),
        ));
    }
    validate_display_name(&code.display_name)?;
    let public =
        decode_hex_fixed::<32>(&code.public_key, "pairing public key must be 32-byte hex")?;
    if node_id_from_public_key(&public) != code.node_id {
        return Err(Error::Protocol(
            "pairing code Node ID does not match public key",
        ));
    }
    let _token = decode_fixed::<32>(&code.token, "pairing token must be 32 bytes")?;
    let endpoint: SocketAddr = code
        .endpoint
        .parse()
        .map_err(|_| Error::Config("pairing code has an invalid direct endpoint".into()))?;
    if endpoint.port() == 0 || endpoint.ip().is_unspecified() {
        return Err(Error::Config(
            "pairing code has an unusable direct endpoint".into(),
        ));
    }
    let cert = STANDARD
        .decode(&code.quic_cert_der)
        .map_err(|_| Error::Protocol("pairing code certificate is not valid base64"))?;
    if cert.is_empty() || cert.len() > 16 * 1024 {
        return Err(Error::Protocol("pairing code certificate has invalid size"));
    }
    let now = now_secs();
    if code.expires_at <= now {
        return Err(Error::Config("AwNet pairing code has expired".into()));
    }
    if code.expires_at > now.saturating_add(3600) {
        return Err(Error::Config(
            "AwNet pairing code lifetime is unexpectedly long".into(),
        ));
    }
    Ok(())
}

async fn write_frame<T: Serialize>(send: &mut quinn::SendStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Error::Protocol("cannot encode AwNet pairing frame"))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(Error::Limit("AwNet pairing frame exceeds bounded size"));
    }
    let len = u32::try_from(bytes.len())
        .map_err(|_| Error::Limit("AwNet pairing frame length overflow"))?;
    send.write_all(&len.to_be_bytes())
        .await
        .map_err(|_| Error::Transport("failed to write QUIC pairing frame"))?;
    send.write_all(&bytes)
        .await
        .map_err(|_| Error::Transport("failed to write QUIC pairing frame"))?;
    send.finish()
        .map_err(|_| Error::Transport("failed to finish QUIC pairing stream"))?;
    match send
        .stopped()
        .await
        .map_err(|_| Error::Transport("QUIC pairing stream acknowledgement failed"))?
    {
        None => Ok(()),
        Some(_) => Err(Error::Transport(
            "peer stopped QUIC pairing stream before acknowledging the frame",
        )),
    }
}

const MAX_DATA_FRAME_BYTES: usize = 4 * 1024 * 1024;

async fn write_data_frame<T: Serialize>(send: &mut quinn::SendStream, value: &T) -> Result<()> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| Error::Protocol("cannot encode AwNet data frame"))?;
    if bytes.is_empty() || bytes.len() > MAX_DATA_FRAME_BYTES {
        return Err(Error::Limit("AwNet data frame exceeds 4 MiB"));
    }
    let len =
        u32::try_from(bytes.len()).map_err(|_| Error::Limit("AwNet data frame length overflow"))?;
    send.write_all(&len.to_be_bytes())
        .await
        .map_err(|_| Error::Transport("failed to write QUIC data frame"))?;
    send.write_all(&bytes)
        .await
        .map_err(|_| Error::Transport("failed to write QUIC data frame"))?;
    send.finish()
        .map_err(|_| Error::Transport("failed to finish QUIC data stream"))?;
    match send
        .stopped()
        .await
        .map_err(|_| Error::Transport("QUIC data stream acknowledgement failed"))?
    {
        None => Ok(()),
        Some(_) => Err(Error::Transport(
            "peer stopped QUIC data stream before acknowledging the frame",
        )),
    }
}

async fn read_data_frame<T: for<'de> Deserialize<'de>>(recv: &mut quinn::RecvStream) -> Result<T> {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|_| Error::Transport("failed to read QUIC data frame"))?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_DATA_FRAME_BYTES {
        return Err(Error::Limit("AwNet data frame exceeds 4 MiB"));
    }
    let mut bytes = vec![0u8; length];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|_| Error::Transport("failed to read QUIC data frame"))?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol("invalid AwNet data frame"))
}

async fn read_frame<T: for<'de> Deserialize<'de>>(recv: &mut quinn::RecvStream) -> Result<T> {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|_| Error::Transport("failed to read QUIC pairing frame"))?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(Error::Limit("AwNet pairing frame exceeds bounded size"));
    }
    let mut bytes = vec![0u8; length];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|_| Error::Transport("failed to read QUIC pairing frame"))?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol("invalid AwNet pairing frame"))
}

impl PairListener {
    fn bind(
        store: &SessionStore,
        state_dir: &Path,
        opts: OfferOptions,
    ) -> Result<(Self, PairOffer)> {
        validate_display_name(&opts.display_name)?;
        if opts.timeout < Duration::from_secs(10) || opts.timeout > Duration::from_secs(900) {
            return Err(Error::Config(
                "pairing timeout must be between 10 and 900 seconds".into(),
            ));
        }
        let identity = ensure_identity(store, state_dir)?;
        let endpoint = Endpoint::server(server_config(&identity)?, opts.listen)
            .map_err(|_| Error::Transport("cannot bind direct QUIC pairing listener"))?;
        let bound = endpoint
            .local_addr()
            .map_err(|_| Error::Transport("cannot inspect QUIC pairing listener address"))?;
        let mut advertise = opts.advertise.unwrap_or_else(|| discover_advertise(bound));
        if advertise.port() == 0 {
            advertise.set_port(bound.port());
        }
        if advertise.port() == 0 || advertise.ip().is_unspecified() {
            return Err(Error::Config(
                "--advertise must resolve to a concrete address and port".into(),
            ));
        }
        let mut token = [0u8; 32];
        getrandom::fill(&mut token)
            .map_err(|_| Error::Storage("OS randomness unavailable for pairing capability"))?;
        let expires_at = now_secs().saturating_add(opts.timeout.as_secs());
        let code = PairingCode {
            version: PAIR_VERSION,
            node_id: identity.node_id.clone(),
            public_key: identity.public_key.clone(),
            display_name: opts.display_name,
            endpoint: advertise.to_string(),
            token: STANDARD.encode(token),
            expires_at,
            quic_cert_der: STANDARD.encode(&identity.quic_cert_der),
        };
        validate_pairing_code(&code)?;
        let encoded = encode_pairing_code(&code)?;
        let offer = PairOffer {
            code: encoded,
            node_id: identity.node_id.clone(),
            endpoint: advertise,
            expires_at,
        };
        let deadline = tokio::time::Instant::now() + opts.timeout;
        Ok((
            Self {
                endpoint,
                identity,
                code,
                deadline,
            },
            offer,
        ))
    }

    async fn accept(self, store: &SessionStore) -> Result<AwNetPeerInfo> {
        let token = decode_fixed::<32>(&self.code.token, "pairing token must be 32 bytes")?;
        for _ in 0..MAX_PAIR_ATTEMPTS {
            let incoming =
                match tokio::time::timeout_at(self.deadline, self.endpoint.accept()).await {
                    Ok(Some(incoming)) => incoming,
                    Ok(None) => return Err(Error::Transport("QUIC pairing listener closed")),
                    Err(_) => return Err(Error::Timeout),
                };
            let connection = match tokio::time::timeout(FRAME_TIMEOUT, incoming).await {
                Ok(Ok(connection)) => connection,
                _ => continue,
            };
            let streams = match tokio::time::timeout(FRAME_TIMEOUT, connection.accept_bi()).await {
                Ok(Ok(streams)) => streams,
                _ => {
                    connection.close(1u32.into(), b"pairing stream required");
                    continue;
                }
            };
            let (mut send, mut recv) = streams;
            let request: PairRequest =
                match tokio::time::timeout(FRAME_TIMEOUT, read_frame(&mut recv)).await {
                    Ok(Ok(request)) => request,
                    _ => {
                        connection.close(1u32.into(), b"invalid pairing request");
                        continue;
                    }
                };
            match self.validate_request(&request, &token) {
                Ok((request_nonce, peer)) => {
                    let trusted = match store.trust_awnet_peer(&peer.0, &peer.1, &peer.2) {
                        Ok(value) => value,
                        Err(error) => {
                            let response =
                                PairResponse::rejected("peer trust could not be persisted");
                            let _ = tokio::time::timeout(
                                FRAME_TIMEOUT,
                                write_frame(&mut send, &response),
                            )
                            .await;
                            connection.close(1u32.into(), b"pairing persistence denied");
                            return Err(error);
                        }
                    };
                    let mut response_nonce = [0u8; 32];
                    getrandom::fill(&mut response_nonce).map_err(|_| {
                        Error::Storage("OS randomness unavailable for pairing response")
                    })?;
                    let message = response_transcript(
                        &token,
                        &self.identity.node_id,
                        &request.node_id,
                        &request_nonce,
                        &response_nonce,
                    );
                    let signature = self.identity.signing.sign(&message);
                    let response = PairResponse {
                        version: PAIR_VERSION,
                        accepted: true,
                        node_id: Some(self.identity.node_id.clone()),
                        public_key: Some(self.identity.public_key.clone()),
                        display_name: Some(self.code.display_name.clone()),
                        request_nonce: Some(STANDARD.encode(request_nonce)),
                        nonce: Some(STANDARD.encode(response_nonce)),
                        signature: Some(STANDARD.encode(signature.to_bytes())),
                        error: None,
                    };
                    tokio::time::timeout(FRAME_TIMEOUT, write_frame(&mut send, &response))
                        .await
                        .map_err(|_| Error::Timeout)??;
                    connection.close(0u32.into(), b"paired");
                    self.endpoint.close(0u32.into(), b"pairing complete");
                    self.endpoint.wait_idle().await;
                    return Ok(trusted);
                }
                Err(_) => {
                    let response = PairResponse::rejected("pairing request rejected");
                    let _ = tokio::time::timeout(FRAME_TIMEOUT, write_frame(&mut send, &response))
                        .await;
                    connection.close(1u32.into(), b"pairing rejected");
                }
            }
        }
        Err(Error::Denied("too many invalid direct pairing attempts"))
    }

    fn validate_request(
        &self,
        request: &PairRequest,
        token: &[u8; 32],
    ) -> Result<([u8; 32], (String, String, String))> {
        if request.version != PAIR_VERSION
            || request.inviter_node_id != self.identity.node_id
            || request.node_id == self.identity.node_id
        {
            return Err(Error::Protocol("invalid AwNet pairing request envelope"));
        }
        validate_display_name(&request.display_name)?;
        let supplied_token = decode_fixed::<32>(&request.token, "pairing token must be 32 bytes")?;
        if supplied_token.ct_eq(token).unwrap_u8() != 1 {
            return Err(Error::Denied("invalid direct pairing capability"));
        }
        let nonce = decode_fixed::<32>(&request.nonce, "pairing nonce must be 32 bytes")?;
        let message = request_transcript(token, &self.identity.node_id, &request.node_id, &nonce);
        verify_signed_identity(
            &request.node_id,
            &request.public_key,
            &message,
            &request.signature,
        )?;
        Ok((
            nonce,
            (
                request.node_id.clone(),
                request.public_key.clone(),
                request.display_name.clone(),
            ),
        ))
    }
}

impl PairResponse {
    fn rejected(message: &str) -> Self {
        Self {
            version: PAIR_VERSION,
            accepted: false,
            node_id: None,
            public_key: None,
            display_name: None,
            request_nonce: None,
            nonce: None,
            signature: None,
            error: Some(message.to_owned()),
        }
    }
}

async fn accept_pairing_code_async(
    store: &SessionStore,
    state_dir: &Path,
    code: &str,
    display_name: &str,
) -> Result<AwNetPeerInfo> {
    validate_display_name(display_name)?;
    let code = decode_pairing_code(code)?;
    let identity = ensure_identity(store, state_dir)?;
    if code.node_id == identity.node_id {
        return Err(Error::Denied("cannot pair an AwNode with itself"));
    }
    let endpoint_addr: SocketAddr = code
        .endpoint
        .parse()
        .map_err(|_| Error::Config("invalid direct pairing endpoint".into()))?;
    let cert_der = STANDARD
        .decode(&code.quic_cert_der)
        .map_err(|_| Error::Protocol("invalid pairing certificate"))?;
    let bind = match endpoint_addr.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let mut endpoint = Endpoint::client(bind)
        .map_err(|_| Error::Transport("cannot create QUIC pairing client"))?;
    endpoint.set_default_client_config(client_config(&cert_der)?);
    let connecting = endpoint
        .connect(endpoint_addr, PAIR_SERVER_NAME)
        .map_err(|_| Error::Transport("cannot start certificate-pinned QUIC pairing connection"))?;
    let connection = tokio::time::timeout(FRAME_TIMEOUT, connecting)
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Transport("certificate-pinned QUIC pairing connection failed"))?;
    let (mut send, mut recv) = tokio::time::timeout(FRAME_TIMEOUT, connection.open_bi())
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Transport("cannot open QUIC pairing stream"))?;
    let token = decode_fixed::<32>(&code.token, "pairing token must be 32 bytes")?;
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for pairing request"))?;
    let message = request_transcript(&token, &code.node_id, &identity.node_id, &nonce);
    let signature = identity.signing.sign(&message);
    let request = PairRequest {
        version: PAIR_VERSION,
        inviter_node_id: code.node_id.clone(),
        node_id: identity.node_id.clone(),
        public_key: identity.public_key.clone(),
        display_name: display_name.to_owned(),
        token: code.token.clone(),
        nonce: STANDARD.encode(nonce),
        signature: STANDARD.encode(signature.to_bytes()),
    };
    tokio::time::timeout(FRAME_TIMEOUT, write_frame(&mut send, &request))
        .await
        .map_err(|_| Error::Timeout)??;
    let response: PairResponse = tokio::time::timeout(FRAME_TIMEOUT, read_frame(&mut recv))
        .await
        .map_err(|_| Error::Timeout)??;
    if response.version != PAIR_VERSION || !response.accepted {
        return Err(Error::Denied("direct AwNet pairing was rejected"));
    }
    let response_node = response
        .node_id
        .as_deref()
        .ok_or(Error::Protocol("pair response missing node ID"))?;
    let response_public = response
        .public_key
        .as_deref()
        .ok_or(Error::Protocol("pair response missing public key"))?;
    let response_name = response
        .display_name
        .as_deref()
        .ok_or(Error::Protocol("pair response missing display name"))?;
    if response_node != code.node_id || response_public != code.public_key {
        return Err(Error::Protocol(
            "pair response identity differs from the invitation",
        ));
    }
    validate_display_name(response_name)?;
    let echoed_nonce = decode_fixed::<32>(
        response
            .request_nonce
            .as_deref()
            .ok_or(Error::Protocol("pair response missing request nonce"))?,
        "pair response request nonce must be 32 bytes",
    )?;
    if echoed_nonce != nonce {
        return Err(Error::Protocol(
            "pair response is not bound to this request",
        ));
    }
    let response_nonce = decode_fixed::<32>(
        response
            .nonce
            .as_deref()
            .ok_or(Error::Protocol("pair response missing nonce"))?,
        "pair response nonce must be 32 bytes",
    )?;
    let response_message = response_transcript(
        &token,
        response_node,
        &identity.node_id,
        &nonce,
        &response_nonce,
    );
    verify_signed_identity(
        response_node,
        response_public,
        &response_message,
        response
            .signature
            .as_deref()
            .ok_or(Error::Protocol("pair response missing signature"))?,
    )?;
    let trusted = store.trust_awnet_peer(response_node, response_public, response_name)?;
    connection.close(0u32.into(), b"paired");
    endpoint.close(0u32.into(), b"pairing complete");
    endpoint.wait_idle().await;
    Ok(trusted)
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::from)
}

pub fn status(store: &SessionStore, state_dir: &Path) -> Result<serde_json::Value> {
    let path = state_dir.join(IDENTITY_FILE);
    if !path.exists() {
        if store.local_machine()?.key_fingerprint.is_some() {
            return Err(Error::Denied("persisted AwNode identity exists but node-identity.json is missing; restore it instead of rotating identity implicitly"));
        }
        let mut value = store.node_status_with_transport("quic_pairing_available")?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "identity_initialized".into(),
                serde_json::Value::Bool(false),
            );
            object.insert("pairing_protocol".into(), serde_json::json!(PAIR_VERSION));
        }
        return Ok(value);
    }
    let identity = materialize_identity(read_identity_file(&path)?)?;
    let _ = server_config(&identity)?;
    store.bind_local_node_identity(&identity.node_id, &identity.key_fingerprint)?;
    let mut value = store.node_status_with_transport("quic_pairing_ready")?;
    if let Some(object) = value.as_object_mut() {
        object.insert("identity_initialized".into(), serde_json::Value::Bool(true));
        object.insert(
            "public_key".into(),
            serde_json::Value::String(identity.public_key),
        );
        object.insert("pairing_protocol".into(), serde_json::json!(PAIR_VERSION));
    }
    Ok(value)
}

pub fn offer_pair<F>(
    store: &SessionStore,
    state_dir: &Path,
    opts: OfferOptions,
    announce: F,
) -> Result<AwNetPeerInfo>
where
    F: FnOnce(&PairOffer) -> Result<()>,
{
    runtime()?.block_on(async {
        let (listener, offer) = PairListener::bind(store, state_dir, opts)?;
        announce(&offer)?;
        listener.accept(store).await
    })
}

pub fn accept_pairing_code(
    store: &SessionStore,
    state_dir: &Path,
    code: &str,
    display_name: &str,
) -> Result<AwNetPeerInfo> {
    runtime()?.block_on(accept_pairing_code_async(
        store,
        state_dir,
        code,
        display_name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(label: &str) -> PathBuf {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        std::env::temp_dir().join(format!(
            "aw-{label}-{}-{}",
            std::process::id(),
            hex(&random)
        ))
    }

    #[test]
    fn node_id_is_stable_key_hash() {
        let seed = [7u8; 32];
        let signing = SigningKey::from_bytes(&seed);
        let public = signing.verifying_key().to_bytes();
        assert_eq!(
            node_id_from_public_key(&public),
            node_id_from_public_key(&public)
        );
        assert!(node_id_from_public_key(&public).starts_with("node_"));
        assert_eq!(fingerprint_from_public_key(&public).len(), 71);
    }

    #[test]
    fn pairing_code_rejects_tampered_node_id() {
        let mut code = PairingCode {
            version: PAIR_VERSION,
            node_id: format!("node_{}", "0".repeat(64)),
            public_key: hex(&[1u8; 32]),
            display_name: "peer".into(),
            endpoint: "127.0.0.1:4433".into(),
            token: STANDARD.encode([2u8; 32]),
            expires_at: now_secs() + 60,
            quic_cert_der: STANDARD.encode([3u8; 8]),
        };
        assert!(validate_pairing_code(&code).is_err());
        code.node_id = node_id_from_public_key(&[1u8; 32]);
        assert!(validate_pairing_code(&code).is_ok());
    }

    #[test]
    fn persisted_identity_is_stable_and_store_bound() {
        let dir = temp_state("identity");
        let store = SessionStore::open(&dir).unwrap();
        let first = ensure_identity(&store, &dir).unwrap();
        let second = ensure_identity(&store, &dir).unwrap();
        assert_eq!(first.node_id, second.node_id);
        assert_eq!(store.local_machine().unwrap().node_id, first.node_id);
        assert_eq!(
            store.local_machine().unwrap().key_fingerprint.as_deref(),
            Some(first.key_fingerprint.as_str())
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn localhost_quic_pairing_persists_both_peer_keys() {
        let a_dir = temp_state("pair-a");
        let b_dir = temp_state("pair-b");
        let a = SessionStore::open(&a_dir).unwrap();
        let b = SessionStore::open(&b_dir).unwrap();
        let opts = OfferOptions {
            listen: "127.0.0.1:0".parse().unwrap(),
            advertise: None,
            display_name: "node-a".into(),
            timeout: Duration::from_secs(30),
        };
        let (listener, offer) = PairListener::bind(&a, &a_dir, opts).unwrap();
        let server = listener.accept(&a);
        let client = accept_pairing_code_async(&b, &b_dir, &offer.code, "node-b");
        let (a_peer, b_peer) = tokio::join!(server, client);
        let a_peer = a_peer.unwrap();
        let b_peer = b_peer.unwrap();
        assert_eq!(a_peer.node_id, b.local_machine().unwrap().node_id);
        assert_eq!(b_peer.node_id, a.local_machine().unwrap().node_id);
        assert_eq!(a.awnet_peers().unwrap().len(), 1);
        assert_eq!(b.awnet_peers().unwrap().len(), 1);
        drop(a);
        drop(b);
        std::fs::remove_dir_all(a_dir).unwrap();
        std::fs::remove_dir_all(b_dir).unwrap();
    }
}
