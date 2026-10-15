use super::machinelink::{
    negotiate_client, negotiate_server, MachineCapability, MachineLinkConnectOptions,
    MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_store::{
    identity_continuity_snapshot_transcript, AuthSessionInfo, AuthenticatedStableUser,
    IdentityContinuityApplyResult, IdentityContinuitySnapshot, SessionInfo,
};
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
use zeroize::{Zeroize, Zeroizing};

const IDENTITY_SYNC_STREAM_VERSION: u16 = 1;
const IDENTITY_AUTH_STREAM_VERSION: u16 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityContinuityRequest {
    domain_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityContinuityEnvelope {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    request: IdentityContinuityRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityContinuityReply {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    snapshot: IdentityContinuitySnapshot,
    source_signature: String,
}

#[derive(Clone, Debug)]
pub struct IdentityContinuityServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
    pub domain_id: String,
}

#[derive(Clone, Debug)]
pub struct IdentityContinuitySyncOptions {
    pub link: MachineLinkConnectOptions,
    pub actor: PrincipalId,
    pub domain_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentityContinuityServeResult {
    pub lease: MachineLinkLease,
    pub domain_id: String,
    pub snapshot_sha256: String,
    pub users: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentityContinuitySyncResult {
    pub lease: MachineLinkLease,
    pub applied: IdentityContinuityApplyResult,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityAuthHello {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    domain_id: String,
    public_key: String,
    session_id: Option<String>,
    auth_session_ttl: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityAuthChallenge {
    pub version: u16,
    pub machine_lease_id: String,
    pub request_id: String,
    pub domain_id: String,
    pub target_node_id: String,
    pub target_store_id: String,
    pub peer_node_id: String,
    pub nonce: String,
    pub session_id: Option<String>,
    pub auth_session_ttl: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityAuthProof {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    public_key: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityAuthSessionWire {
    info: AuthSessionInfo,
    refresh_token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityAuthReply {
    pub version: u16,
    pub machine_lease_id: String,
    pub request_id: String,
    pub authenticated: AuthenticatedStableUser,
    pub session: Option<SessionInfo>,
    auth_session: Option<IdentityAuthSessionWire>,
}

pub struct IdentityAuthRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub domain_id: String,
    pub session_id: Option<String>,
    pub auth_session_ttl: Option<u64>,
    pub signing_key: SigningKey,
}

#[derive(Clone, Debug)]
pub struct IdentityAuthServeOptions {
    pub link: MachineLinkServeOptions,
    pub domain_id: String,
    pub auth_session_max_ttl: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentityAuthServeResult {
    pub lease: MachineLinkLease,
    pub authenticated_user_id: String,
    pub session_authorized: bool,
    pub auth_session_issued: bool,
}

#[derive(Serialize)]
pub struct IdentityAuthRequestResult {
    pub lease: MachineLinkLease,
    pub authenticated: AuthenticatedStableUser,
    pub session: Option<SessionInfo>,
    pub auth_session: Option<AuthSessionInfo>,
    #[serde(skip_serializing)]
    pub refresh_token: Option<Zeroizing<String>>,
}

fn request_id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for identity request"))?;
    Ok(format!("{prefix}_{}", hex(&bytes)))
}

fn nonce() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for identity challenge"))?;
    Ok(hex(&bytes))
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

pub fn identity_device_auth_transcript(
    challenge: &IdentityAuthChallenge,
    public_key: &str,
) -> Vec<u8> {
    let session = challenge.session_id.as_deref().unwrap_or("");
    let ttl = challenge
        .auth_session_ttl
        .map(|value| value.to_string())
        .unwrap_or_default();
    let fields: [&[u8]; 10] = [
        challenge.machine_lease_id.as_bytes(),
        challenge.request_id.as_bytes(),
        challenge.domain_id.as_bytes(),
        challenge.target_node_id.as_bytes(),
        challenge.target_store_id.as_bytes(),
        challenge.peer_node_id.as_bytes(),
        challenge.nonce.as_bytes(),
        public_key.as_bytes(),
        session.as_bytes(),
        ttl.as_bytes(),
    ];
    let mut out = Vec::with_capacity(544);
    out.extend_from_slice(b"aw-user-device-auth-v2\0");
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

pub fn serve_identity_continuity(
    store: &SessionStore,
    state_dir: &Path,
    opts: IdentityContinuityServeOptions,
) -> Result<IdentityContinuityServeResult> {
    if !opts
        .link
        .allowed
        .contains(&MachineCapability::IdentityContinuity)
    {
        return Err(Error::Config(
            "identity continuity server must explicitly grant identity-continuity".into(),
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let snapshot =
        store.export_identity_continuity_snapshot(&opts.actor, &opts.domain_id, &peer.node_id)?;
    if snapshot.source_node_id != identity.node_id {
        return Err(Error::Conflict(
            "identity continuity source does not match this AwNode",
        ));
    }
    let source_signature = STANDARD.encode(
        identity
            .signing
            .sign(&identity_continuity_snapshot_transcript(&snapshot))
            .to_bytes(),
    );
    let users = snapshot.users.len();
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
            MachineCapability::IdentityContinuity,
            "MachineLink lease does not grant identity continuity",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("identity continuity stream was not opened"))?;
        let envelope: IdentityContinuityEnvelope = read_data_frame(&mut recv).await?;
        if envelope.version != IDENTITY_SYNC_STREAM_VERSION
            || envelope.machine_lease_id != lease.lease_id
            || envelope.request.domain_id != snapshot.domain_id
        {
            return Err(Error::Protocol(
                "identity continuity request is not bound to the negotiated lease/Domain",
            ));
        }
        write_data_frame(
            &mut send,
            &IdentityContinuityReply {
                version: IDENTITY_SYNC_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id,
                snapshot: snapshot.clone(),
                source_signature: source_signature.clone(),
            },
        )
        .await?;
        Ok::<_, Error>(IdentityContinuityServeResult {
            lease,
            domain_id: snapshot.domain_id.clone(),
            snapshot_sha256: snapshot.snapshot_sha256.clone(),
            users,
        })
    });
    link.connection
        .close(0u32.into(), b"identity continuity complete");
    link.endpoint
        .close(0u32.into(), b"identity continuity complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_identity_continuity(
    store: &SessionStore,
    state_dir: &Path,
    opts: IdentityContinuitySyncOptions,
) -> Result<IdentityContinuitySyncResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::IdentityContinuity)
    {
        return Err(Error::Config(
            "identity continuity client must explicitly request identity-continuity".into(),
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
            MachineCapability::IdentityContinuity,
            "MachineLink lease does not grant identity continuity",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open identity continuity stream"))?;
        let request_id = request_id("icrq")?;
        write_data_frame(
            &mut send,
            &IdentityContinuityEnvelope {
                version: IDENTITY_SYNC_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request: IdentityContinuityRequest {
                    domain_id: opts.domain_id.clone(),
                },
            },
        )
        .await?;
        let reply: IdentityContinuityReply = read_data_frame(&mut recv).await?;
        if reply.version != IDENTITY_SYNC_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
            || reply.snapshot.domain_id != opts.domain_id
            || reply.snapshot.source_node_id != peer.node_id
        {
            return Err(Error::Protocol(
                "identity continuity response correlation failed",
            ));
        }
        verify_signed_identity(
            &peer.node_id,
            &peer.public_key,
            &identity_continuity_snapshot_transcript(&reply.snapshot),
            &reply.source_signature,
        )?;
        let applied = store.import_identity_continuity_snapshot(
            &opts.actor,
            &peer.node_id,
            &reply.snapshot,
            &reply.source_signature,
        )?;
        Ok::<_, Error>(IdentityContinuitySyncResult { lease, applied })
    });
    link.connection
        .close(0u32.into(), b"identity continuity complete");
    link.endpoint
        .close(0u32.into(), b"identity continuity complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn serve_identity_auth(
    store: &SessionStore,
    state_dir: &Path,
    opts: IdentityAuthServeOptions,
) -> Result<IdentityAuthServeResult> {
    if opts
        .auth_session_max_ttl
        .is_some_and(|ttl| !(60..=30 * 24 * 60 * 60).contains(&ttl))
    {
        return Err(Error::Config(
            "identity auth native-session max TTL must be between 60 seconds and 30 days".into(),
        ));
    }
    if !opts
        .link
        .allowed
        .contains(&MachineCapability::IdentityAuthenticate)
    {
        return Err(Error::Config(
            "identity auth server must explicitly grant identity-authenticate".into(),
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
        require_capability(
            &lease.capabilities,
            MachineCapability::IdentityAuthenticate,
            "MachineLink lease does not grant identity authentication",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("identity auth stream was not opened"))?;
        let hello: IdentityAuthHello = read_data_frame(&mut recv).await?;
        if hello.version != IDENTITY_AUTH_STREAM_VERSION
            || hello.machine_lease_id != lease.lease_id
            || hello.domain_id != opts.domain_id
        {
            return Err(Error::Protocol(
                "identity auth hello is not bound to the negotiated lease/Domain",
            ));
        }
        if let Some(ttl) = hello.auth_session_ttl {
            let max = opts.auth_session_max_ttl.ok_or(Error::Denied(
                "identity auth server did not enable native auth-session issuance",
            ))?;
            if ttl < 60 || ttl > max || max > 30 * 24 * 60 * 60 {
                return Err(Error::Denied(
                    "requested native auth-session TTL exceeds the server policy",
                ));
            }
        }
        let challenge = IdentityAuthChallenge {
            version: IDENTITY_AUTH_STREAM_VERSION,
            machine_lease_id: lease.lease_id.clone(),
            request_id: hello.request_id.clone(),
            domain_id: opts.domain_id.clone(),
            target_node_id: identity.node_id.clone(),
            target_store_id: target_store_id.clone(),
            peer_node_id: peer.node_id.clone(),
            nonce: nonce()?,
            session_id: hello.session_id.clone(),
            auth_session_ttl: hello.auth_session_ttl,
        };
        write_data_frame(&mut send, &challenge).await?;
        let (mut proof_send, mut proof_recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.accept_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("identity auth proof stream was not opened"))?;
        let proof: IdentityAuthProof = read_data_frame(&mut proof_recv).await?;
        if proof.version != IDENTITY_AUTH_STREAM_VERSION
            || proof.machine_lease_id != lease.lease_id
            || proof.request_id != hello.request_id
            || proof.public_key != hello.public_key
        {
            return Err(Error::Protocol("identity auth proof correlation failed"));
        }
        let transcript = identity_device_auth_transcript(&challenge, &proof.public_key);
        let authenticated = store.authenticate_portable_device_signature(
            &opts.domain_id,
            &proof.public_key,
            &transcript,
            &proof.signature,
        )?;
        let principal = PrincipalId::new(authenticated.stable_principal.clone())?;
        let session = hello
            .session_id
            .as_deref()
            .map(|session_id| store.session(&principal, session_id))
            .transpose()?;
        let issued = hello
            .auth_session_ttl
            .map(|ttl| {
                store.issue_auth_session_from_portable_auth(
                    &authenticated,
                    ttl,
                    serde_json::json!({
                        "client":"awnet-device-auth",
                        "peer_node_id":peer.node_id,
                        "machine_lease_id":lease.lease_id
                    }),
                )
            })
            .transpose()?;
        let wire_session = issued.as_ref().map(|issued| IdentityAuthSessionWire {
            info: issued.info.clone(),
            refresh_token: issued.refresh_token.as_str().to_owned(),
        });
        let mut reply = IdentityAuthReply {
            version: IDENTITY_AUTH_STREAM_VERSION,
            machine_lease_id: lease.lease_id.clone(),
            request_id: hello.request_id,
            authenticated: authenticated.clone(),
            session: session.clone(),
            auth_session: wire_session,
        };
        let send_result = write_data_frame(&mut proof_send, &reply).await;
        if let Some(auth_session) = reply.auth_session.as_mut() {
            auth_session.refresh_token.zeroize();
        }
        if let Err(error) = send_result {
            if let Some(issued) = &issued {
                let _ = store.revoke_auth_session_by_token(&issued.refresh_token);
            }
            return Err(error);
        }
        Ok::<_, Error>(IdentityAuthServeResult {
            lease,
            authenticated_user_id: authenticated.user_id,
            session_authorized: session.is_some(),
            auth_session_issued: issued.is_some(),
        })
    });
    link.connection
        .close(0u32.into(), b"identity auth complete");
    link.endpoint.close(0u32.into(), b"identity auth complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_identity_auth(
    store: &SessionStore,
    state_dir: &Path,
    opts: IdentityAuthRequestOptions,
) -> Result<IdentityAuthRequestResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::IdentityAuthenticate)
    {
        return Err(Error::Config(
            "identity auth client must explicitly request identity-authenticate".into(),
        ));
    }
    let peer = store.awnet_peer_by_node_id(&opts.link.route.peer_node_id)?;
    let identity = ensure_identity(store, state_dir)?;
    let public_key = hex(&opts.signing_key.verifying_key().to_bytes());
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
            MachineCapability::IdentityAuthenticate,
            "MachineLink lease does not grant identity authentication",
        )?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open identity auth stream"))?;
        let request_id = request_id("iarq")?;
        write_data_frame(
            &mut send,
            &IdentityAuthHello {
                version: IDENTITY_AUTH_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                domain_id: opts.domain_id.clone(),
                public_key: public_key.clone(),
                session_id: opts.session_id.clone(),
                auth_session_ttl: opts.auth_session_ttl,
            },
        )
        .await?;
        let challenge: IdentityAuthChallenge = read_data_frame(&mut recv).await?;
        if challenge.version != IDENTITY_AUTH_STREAM_VERSION
            || challenge.machine_lease_id != lease.lease_id
            || challenge.request_id != request_id
            || challenge.domain_id != opts.domain_id
            || challenge.target_node_id != peer.node_id
            || challenge.peer_node_id != identity.node_id
            || challenge.session_id != opts.session_id
            || challenge.auth_session_ttl != opts.auth_session_ttl
        {
            return Err(Error::Protocol(
                "identity auth challenge correlation failed",
            ));
        }
        let signature = STANDARD.encode(
            opts.signing_key
                .sign(&identity_device_auth_transcript(&challenge, &public_key))
                .to_bytes(),
        );
        let (mut proof_send, mut proof_recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open identity auth proof stream"))?;
        write_data_frame(
            &mut proof_send,
            &IdentityAuthProof {
                version: IDENTITY_AUTH_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                public_key,
                signature,
            },
        )
        .await?;
        let reply: IdentityAuthReply = read_data_frame(&mut proof_recv).await?;
        if reply.version != IDENTITY_AUTH_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
        {
            return Err(Error::Protocol("identity auth reply correlation failed"));
        }
        let (auth_session, refresh_token) = match reply.auth_session {
            Some(grant) => (Some(grant.info), Some(Zeroizing::new(grant.refresh_token))),
            None => (None, None),
        };
        Ok::<_, Error>(IdentityAuthRequestResult {
            lease,
            authenticated: reply.authenticated,
            session: reply.session,
            auth_session,
            refresh_token,
        })
    });
    link.connection
        .close(0u32.into(), b"identity auth complete");
    link.endpoint.close(0u32.into(), b"identity auth complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_transcript_binds_machine_domain_store_and_session() {
        let challenge = IdentityAuthChallenge {
            version: IDENTITY_AUTH_STREAM_VERSION,
            machine_lease_id: "ml_fixture".into(),
            request_id: "iarq_fixture".into(),
            domain_id: "dom_fixture".into(),
            target_node_id: "node_target".into(),
            target_store_id: "store_target".into(),
            peer_node_id: "node_client".into(),
            nonce: "aa".repeat(32),
            session_id: Some("s_fixture".into()),
            auth_session_ttl: Some(600),
        };
        let a = identity_device_auth_transcript(&challenge, &"11".repeat(32));
        let mut changed = challenge.clone();
        changed.session_id = Some("s_other".into());
        let b = identity_device_auth_transcript(&changed, &"11".repeat(32));
        assert_ne!(a, b);
        let mut ttl_changed = challenge.clone();
        ttl_changed.auth_session_ttl = Some(601);
        assert_ne!(
            a,
            identity_device_auth_transcript(&ttl_changed, &"11".repeat(32))
        );
    }
}
