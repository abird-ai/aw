use super::machinelink::{
    negotiate_client, negotiate_server, MachineCapability, MachineLinkConnectOptions,
    MachineLinkLease, MachineLinkServeOptions,
};
use super::reachability::open_punched_quic;
use super::*;
use aw::PrincipalId;
use aw_store::{SemanticSyncApply, SemanticSyncBatch};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

const FEDERATED_STREAM_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FederationRequest {
    SemanticSyncExport {
        federation_lease_id: String,
        after_seq: u64,
        limit: usize,
    },
    SemanticSyncImport {
        federation_lease_id: String,
        batch: SemanticSyncBatch,
    },
}

impl FederationRequest {
    pub fn required_capability(&self) -> MachineCapability {
        match self {
            Self::SemanticSyncExport { .. } => MachineCapability::SemanticSyncRead,
            Self::SemanticSyncImport { .. } => MachineCapability::SemanticSyncWrite,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FederationResponse {
    SemanticSyncBatch { batch: SemanticSyncBatch },
    SemanticSyncApplied { result: SemanticSyncApply },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FederationEnvelope {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    request: FederationRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FederationReply {
    version: u16,
    machine_lease_id: String,
    request_id: String,
    response: FederationResponse,
}

#[derive(Clone, Debug)]
pub struct FederationServeOptions {
    pub link: MachineLinkServeOptions,
    pub actor: PrincipalId,
}

#[derive(Clone, Debug)]
pub struct FederationRequestOptions {
    pub link: MachineLinkConnectOptions,
    pub request: FederationRequest,
}

#[derive(Clone, Debug, Serialize)]
pub struct FederationServeResult {
    pub lease: MachineLinkLease,
    pub response: FederationResponse,
}

#[derive(Clone, Debug, Serialize)]
pub struct FederationRequestResult {
    pub lease: MachineLinkLease,
    pub response: FederationResponse,
}

fn random_request_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for federation request"))?;
    Ok(format!("frq_{}", hex(&bytes)))
}

fn ensure_capability(
    capabilities: &[MachineCapability],
    required: MachineCapability,
) -> Result<()> {
    if capabilities.contains(&required) {
        Ok(())
    } else {
        Err(Error::Denied(
            "MachineLink lease does not grant this federated operation",
        ))
    }
}

fn execute_federated_request(
    store: &SessionStore,
    actor: &PrincipalId,
    peer_node_id: &str,
    request: FederationRequest,
) -> Result<FederationResponse> {
    match request {
        FederationRequest::SemanticSyncExport {
            federation_lease_id,
            after_seq,
            limit,
        } => Ok(FederationResponse::SemanticSyncBatch {
            batch: store.semantic_sync_export(
                actor,
                &federation_lease_id,
                peer_node_id,
                after_seq,
                limit,
            )?,
        }),
        FederationRequest::SemanticSyncImport {
            federation_lease_id,
            batch,
        } => Ok(FederationResponse::SemanticSyncApplied {
            result: store.semantic_sync_apply(&federation_lease_id, peer_node_id, &batch)?,
        }),
    }
}

pub fn serve_federation_request(
    store: &SessionStore,
    state_dir: &Path,
    opts: FederationServeOptions,
) -> Result<FederationServeResult> {
    if !opts.link.allowed.iter().any(|cap| {
        matches!(
            cap,
            MachineCapability::SemanticSyncRead | MachineCapability::SemanticSyncWrite
        )
    }) {
        return Err(Error::Config(
            "federation server must explicitly grant a semantic-sync capability".into(),
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
                .map_err(|_| Error::Transport("federation request stream was not opened"))?;
        let envelope: FederationEnvelope = read_data_frame(&mut recv).await?;
        if envelope.version != FEDERATED_STREAM_VERSION
            || envelope.machine_lease_id != lease.lease_id
        {
            return Err(Error::Protocol(
                "federation request is not bound to the negotiated MachineLink lease",
            ));
        }
        let required = envelope.request.required_capability();
        ensure_capability(&lease.capabilities, required)?;
        // The durable federation lease is deliberately separate from the
        // short-lived MachineLink lease. Store validation binds it to the
        // authenticated peer node and rechecks all current authority epochs
        // plus the operation-specific sync.read/sync.write action.
        let response =
            execute_federated_request(store, &opts.actor, &peer.node_id, envelope.request)?;
        write_data_frame(
            &mut send,
            &FederationReply {
                version: FEDERATED_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: envelope.request_id,
                response: response.clone(),
            },
        )
        .await?;
        Ok::<_, Error>(FederationServeResult { lease, response })
    });
    link.connection
        .close(0u32.into(), b"federated MachineLink operation complete");
    link.endpoint
        .close(0u32.into(), b"federated MachineLink operation complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

pub fn request_federation_service(
    store: &SessionStore,
    state_dir: &Path,
    opts: FederationRequestOptions,
) -> Result<FederationRequestResult> {
    let required = opts.request.required_capability();
    if !opts.link.requested.contains(&required) {
        return Err(Error::Config(
            "federation client must explicitly request the operation capability".into(),
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
        ensure_capability(&lease.capabilities, required)?;
        let (mut send, mut recv) =
            tokio::time::timeout(Duration::from_secs(10), link.connection.open_bi())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("cannot open federation request stream"))?;
        let request_id = random_request_id()?;
        write_data_frame(
            &mut send,
            &FederationEnvelope {
                version: FEDERATED_STREAM_VERSION,
                machine_lease_id: lease.lease_id.clone(),
                request_id: request_id.clone(),
                request: opts.request,
            },
        )
        .await?;
        let reply: FederationReply = read_data_frame(&mut recv).await?;
        if reply.version != FEDERATED_STREAM_VERSION
            || reply.machine_lease_id != lease.lease_id
            || reply.request_id != request_id
        {
            return Err(Error::Protocol(
                "federation response correlation failed; do not infer an unobserved effect",
            ));
        }
        Ok::<_, Error>(FederationRequestResult {
            lease,
            response: reply.response,
        })
    });
    link.connection
        .close(0u32.into(), b"federated MachineLink operation complete");
    link.endpoint
        .close(0u32.into(), b"federated MachineLink operation complete");
    link.runtime.block_on(link.endpoint.wait_idle());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_operations_use_separate_typed_capabilities() {
        let read = FederationRequest::SemanticSyncExport {
            federation_lease_id: "lease".into(),
            after_seq: 0,
            limit: 1,
        };
        let write = FederationRequest::SemanticSyncImport {
            federation_lease_id: "lease".into(),
            batch: SemanticSyncBatch {
                source_store_id: "store".into(),
                after_seq: 0,
                through_seq: 0,
                events: vec![],
            },
        };
        assert_eq!(
            read.required_capability(),
            MachineCapability::SemanticSyncRead
        );
        assert_eq!(
            write.required_capability(),
            MachineCapability::SemanticSyncWrite
        );
    }
}
