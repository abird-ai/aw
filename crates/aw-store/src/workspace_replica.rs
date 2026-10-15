use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const WORKSPACE_REPLICA_VERSION: u16 = 3;
const MAX_REPLICA_BYTES: usize = 3 * 1024 * 1024;
const MAX_REPLICA_SESSIONS: usize = 512;
const MAX_REPLICA_WRITER_EVENTS: usize = 2048;
const MAX_REPLICA_REF_EVENTS: usize = 65_536;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaDomain {
    pub domain_id: String,
    pub authority_store_id: String,
    pub parent_domain_id: Option<String>,
    pub name: String,
    pub status: String,
    pub policy_revision: u64,
    pub security_epoch: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaWorkspace {
    pub workspace_id: String,
    pub domain_id: String,
    pub name: String,
    pub source_origin_kind: String,
    pub source_host_machine_id: Option<String>,
    pub policy: serde_json::Value,
    pub defaults: serde_json::Value,
    pub policy_revision: u64,
    pub revision: u64,
    pub created_by_user_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaUser {
    pub user_id: String,
    pub realm_id: String,
    pub status: String,
    pub display: serde_json::Value,
    pub security_epoch: u64,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaMember {
    pub principal: String,
    pub can_append: bool,
    pub principal_user_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaNode {
    pub node: Node,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaSession {
    pub session_id: String,
    pub owner: String,
    pub title: String,
    pub binding: SessionBinding,
    pub root_key: String,
    pub root_node: String,
    pub next_turn: u64,
    pub import_review_needed: bool,
    pub owner_user_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub members: Vec<WorkspaceReplicaMember>,
    pub nodes: Vec<WorkspaceReplicaNode>,
    pub branches: Vec<BranchInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaArtifact {
    pub artifact_id: String,
    pub owner: String,
    pub owner_user_id: Option<String>,
    pub body: serde_json::Value,
    pub created_at: i64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaArtifactSource {
    pub artifact_id: String,
    pub session_id: String,
    pub node_id: String,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaMaterializedFactLink {
    pub artifact_id: String,
    pub materialization_id: String,
    pub ordinal: u64,
    pub fact: crate::durability::SemanticMaterializedFact,
    pub fact_sha256: String,
    pub authorization_policy_id: String,
    pub authorization_policy_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaNodeMaterializedFactLink {
    pub node_id: String,
    pub materialization_id: String,
    pub fact: crate::durability::SemanticMaterializedFact,
    pub fact_sha256: String,
    pub authorization_policy_id: String,
    pub authorization_policy_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaNodeDependency {
    pub node_id: String,
    pub source_session: String,
    pub source_node: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaDerivation {
    pub node_id: String,
    pub source_session: String,
    pub requester: String,
    pub requester_user_id: Option<String>,
    pub grant_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaPromptAttachment {
    pub prompt_node: String,
    pub artifact_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaSourceGrant {
    pub session_id: String,
    pub principal: String,
    pub principal_user_id: Option<String>,
    pub accounts: serde_json::Value,
    pub grant_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaBranchRefEvent {
    pub event_id: String,
    pub branch_id: String,
    pub old_head: Option<String>,
    pub old_revision: Option<u64>,
    pub new_head: String,
    pub new_revision: u64,
    pub actor: String,
    pub client_id: Option<String>,
    pub cause_type: String,
    pub cause_id: Option<String>,
    pub committed_at: i64,
    pub writer_node_id: Option<String>,
    pub writer_epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaSnapshot {
    pub version: u16,
    pub source_store_id: String,
    pub source_node_id: String,
    pub domain: WorkspaceReplicaDomain,
    pub workspace: WorkspaceReplicaWorkspace,
    pub writer: WorkspaceWriterInfo,
    pub ref_writer_epoch: u64,
    pub writer_events: Vec<WorkspaceWriterEventInfo>,
    pub users: Vec<WorkspaceReplicaUser>,
    pub sessions: Vec<WorkspaceReplicaSession>,
    pub artifacts: Vec<WorkspaceReplicaArtifact>,
    pub artifact_sources: Vec<WorkspaceReplicaArtifactSource>,
    pub materialized_fact_links: Vec<WorkspaceReplicaMaterializedFactLink>,
    pub node_materialized_fact_links: Vec<WorkspaceReplicaNodeMaterializedFactLink>,
    pub node_dependencies: Vec<WorkspaceReplicaNodeDependency>,
    pub derivations: Vec<WorkspaceReplicaDerivation>,
    pub prompt_attachments: Vec<WorkspaceReplicaPromptAttachment>,
    pub source_grants: Vec<WorkspaceReplicaSourceGrant>,
    pub private_sessions: Vec<String>,
    pub branch_ref_events: Vec<WorkspaceReplicaBranchRefEvent>,
    pub created_at: i64,
    pub snapshot_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaSnapshotInfo {
    pub id: String,
    pub workspace_id: String,
    pub source_store_id: String,
    pub source_node_id: String,
    pub writer_epoch: u64,
    pub authority_term: u64,
    pub snapshot_sha256: String,
    pub imported_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaApplyResult {
    pub snapshot: WorkspaceReplicaSnapshotInfo,
    pub writer: WorkspaceWriterInfo,
    pub sessions_imported: usize,
    pub nodes_imported: usize,
    pub divergences_preserved: usize,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceReplicaDivergenceInfo {
    pub id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub canonical_branch_id: String,
    pub preserved_branch_id: String,
    pub local_writer_epoch: u64,
    pub incoming_writer_epoch: u64,
    pub local_head: String,
    pub incoming_head: String,
    pub detected_at: i64,
}

fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(json(value)?.as_bytes())))
}

fn node_digest(node: &Node) -> Result<String> {
    digest(&serde_json::json!({
        "version":1,
        "id":node.id,
        "parent_id":node.parent_id,
        "kind":node.kind,
        "actor":node.actor,
        "payload":node.payload,
        "created_at":node.created_at
    }))
}

fn artifact_digest(
    artifact_id: &str,
    owner: &str,
    body: &serde_json::Value,
    created_at: i64,
) -> Result<String> {
    digest(&serde_json::json!({
        "version":1,
        "artifact_id":artifact_id,
        "owner":owner,
        "body":body,
        "created_at":created_at
    }))
}

fn snapshot_hash(snapshot: &WorkspaceReplicaSnapshot) -> Result<String> {
    let mut value = serde_json::to_value(snapshot)
        .map_err(|_| Error::Storage("cannot encode Workspace replica snapshot"))?;
    value
        .as_object_mut()
        .ok_or(Error::Storage("invalid Workspace replica snapshot shape"))?
        .insert(
            "snapshot_sha256".into(),
            serde_json::Value::String(String::new()),
        );
    digest(&value)
}

pub fn workspace_replica_snapshot_transcript(snapshot: &WorkspaceReplicaSnapshot) -> Vec<u8> {
    let fields: Vec<Vec<u8>> = vec![
        snapshot.source_store_id.as_bytes().to_vec(),
        snapshot.source_node_id.as_bytes().to_vec(),
        snapshot.domain.domain_id.as_bytes().to_vec(),
        snapshot.workspace.workspace_id.as_bytes().to_vec(),
        snapshot.writer.writer_node_id.as_bytes().to_vec(),
        snapshot.writer.writer_epoch.to_be_bytes().to_vec(),
        snapshot.writer.authority_term.to_be_bytes().to_vec(),
        snapshot.ref_writer_epoch.to_be_bytes().to_vec(),
        snapshot.snapshot_sha256.as_bytes().to_vec(),
    ];
    let mut out = Vec::with_capacity(512);
    out.extend_from_slice(b"aw-workspace-replica-v3\0");
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&field);
    }
    out
}

fn decode_public_key(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Protocol("invalid Workspace replica public key"));
    }
    let mut out = [0u8; 32];
    let bytes = value.as_bytes();
    for (index, slot) in out.iter_mut().enumerate() {
        let start = index * 2;
        let pair = std::str::from_utf8(&bytes[start..start + 2])
            .map_err(|_| Error::Protocol("invalid Workspace replica public key"))?;
        *slot = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::Protocol("invalid Workspace replica public key"))?;
    }
    Ok(out)
}

fn verify_snapshot_signature(
    node_id: &str,
    public_key: &str,
    snapshot: &WorkspaceReplicaSnapshot,
    signature: &str,
) -> Result<()> {
    let public = decode_public_key(public_key)?;
    if format!("node_{:x}", Sha256::digest(public)) != node_id {
        return Err(Error::Protocol(
            "Workspace replica Node ID does not match its public key",
        ));
    }
    let verifying = VerifyingKey::from_bytes(&public)
        .map_err(|_| Error::Protocol("invalid Workspace replica Ed25519 key"))?;
    let bytes = STANDARD
        .decode(signature)
        .map_err(|_| Error::Protocol("invalid Workspace replica signature"))?;
    let bytes: [u8; 64] = bytes
        .try_into()
        .map_err(|_| Error::Protocol("Workspace replica signature must be 64 bytes"))?;
    verifying
        .verify_strict(
            &workspace_replica_snapshot_transcript(snapshot),
            &Signature::from_bytes(&bytes),
        )
        .map_err(|_| Error::Denied("Workspace replica signature verification failed"))
}

fn workspace_row_raw(c: &Connection, workspace_id: &str) -> Result<WorkspaceReplicaWorkspace> {
    db(c.query_row(
        "SELECT workspace_id,domain_id,name,origin_kind,host_machine_id,policy_json,defaults_json,policy_revision,revision,created_by_user_id,created_at,updated_at,archived_at,kind FROM workspaces WHERE workspace_id=?1",
        [workspace_id],
        |r| {
            let archived: Option<i64> = r.get(12)?;
            let kind: String = r.get(13)?;
            if archived.is_some() || kind != "hosted" {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let policy: String = r.get(5)?;
            let defaults: String = r.get(6)?;
            Ok((
                r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
                r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,policy,defaults,
                r.get::<_,u64>(7)?,r.get::<_,u64>(8)?,r.get::<_,Option<String>>(9)?,
                r.get::<_,i64>(10)?,r.get::<_,i64>(11)?
            ))
        },
    )).and_then(|r| Ok(WorkspaceReplicaWorkspace {
        workspace_id:r.0, domain_id:r.1, name:r.2, source_origin_kind:r.3,
        source_host_machine_id:r.4, policy:decode(&r.5)?, defaults:decode(&r.6)?,
        policy_revision:r.7, revision:r.8, created_by_user_id:r.9,
        created_at:r.10, updated_at:r.11,
    }))
}

fn domain_row_raw(c: &Connection, domain_id: &str) -> Result<WorkspaceReplicaDomain> {
    let row:(String,String,Option<String>,String,String,u64,u64,i64,i64)=db(c.query_row(
        "SELECT domain_id,authority_store_id,parent_domain_id,name,status,policy_revision,security_epoch,created_at,updated_at FROM domains WHERE domain_id=?1",
        [domain_id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?)),
    ))?;
    Ok(WorkspaceReplicaDomain {
        domain_id: row.0,
        authority_store_id: row.1,
        parent_domain_id: row.2,
        name: row.3,
        status: row.4,
        policy_revision: row.5,
        security_epoch: row.6,
        created_at: row.7,
        updated_at: row.8,
    })
}

fn origin_control_node(c: &Connection, workspace_id: &str) -> Result<Option<String>> {
    db(c.query_row(
        "SELECT control_node_id FROM workspace_replica_origins WHERE workspace_id=?1",
        [workspace_id],
        |r| r.get(0),
    )
    .optional())
}

fn snapshot_info_row(c: &Connection, snapshot_id: &str) -> Result<WorkspaceReplicaSnapshotInfo> {
    db(c.query_row(
        "SELECT snapshot_id,workspace_id,source_store_id,source_node_id,writer_epoch,authority_term,snapshot_sha256,imported_at FROM workspace_replica_snapshots WHERE snapshot_id=?1",
        [snapshot_id],
        |r| Ok(WorkspaceReplicaSnapshotInfo{id:r.get(0)?,workspace_id:r.get(1)?,source_store_id:r.get(2)?,source_node_id:r.get(3)?,writer_epoch:r.get(4)?,authority_term:r.get(5)?,snapshot_sha256:r.get(6)?,imported_at:r.get(7)?}),
    ).optional())?.ok_or(Error::Denied("Workspace replica snapshot unavailable"))
}

fn divergence_rows(
    c: &Connection,
    workspace_id: &str,
) -> Result<Vec<WorkspaceReplicaDivergenceInfo>> {
    let mut q=db(c.prepare("SELECT divergence_id,workspace_id,session_id,canonical_branch_id,preserved_branch_id,local_writer_epoch,incoming_writer_epoch,local_head,incoming_head,detected_at FROM workspace_replica_divergences WHERE workspace_id=?1 ORDER BY detected_at,divergence_id LIMIT 1024"))?;
    let rows = db(q.query_map([workspace_id], |r| {
        Ok(WorkspaceReplicaDivergenceInfo {
            id: r.get(0)?,
            workspace_id: r.get(1)?,
            session_id: r.get(2)?,
            canonical_branch_id: r.get(3)?,
            preserved_branch_id: r.get(4)?,
            local_writer_epoch: r.get(5)?,
            incoming_writer_epoch: r.get(6)?,
            local_head: r.get(7)?,
            incoming_head: r.get(8)?,
            detected_at: r.get(9)?,
        })
    }))?;
    db(rows.collect::<rusqlite::Result<Vec<_>>>())
}

fn require_quiescent(c: &Connection, workspace_id: &str) -> Result<()> {
    let active: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM branches b JOIN sessions s ON s.id=b.session_id WHERE s.workspace_id=?1 AND b.active_run IS NOT NULL)",
        [workspace_id],|r|r.get(0)))?;
    if active {
        return Err(Error::Conflict(
            "Workspace writer handoff requires all branch runs to be quiescent",
        ));
    }
    let runs: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM runs r JOIN sessions s ON s.id=r.session_id WHERE s.workspace_id=?1 AND r.status='running')",
        [workspace_id],|r|r.get(0)))?;
    if runs {
        return Err(Error::Conflict(
            "Workspace writer handoff requires no running execution",
        ));
    }
    let hazards: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM hazards h JOIN sessions s ON s.id=h.session_id WHERE s.workspace_id=?1 AND h.state<>'settled')",
        [workspace_id],|r|r.get(0)))?;
    if hazards {
        return Err(Error::Denied(
            "Workspace writer handoff is blocked by unresolved external-effect hazards",
        ));
    }
    let pending: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_pending p JOIN sessions s ON s.id=p.session_id WHERE s.workspace_id=?1 AND p.state='pending')",
        [workspace_id],|r|r.get(0)))?;
    if pending {
        return Err(Error::Conflict("Workspace writer handoff requires pending context attachments to be consumed or discarded"));
    }
    Ok(())
}

fn require_export_authority(
    c: &Connection,
    actor: &PrincipalId,
    workspace: &WorkspaceReplicaWorkspace,
    writer: &WorkspaceWriterInfo,
    requester_node_id: &str,
) -> Result<()> {
    let local_node: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='local_node_id'",
        [],
        |r| r.get(0),
    ))?;
    let control = origin_control_node(c, &workspace.workspace_id)?;
    if writer.writer_node_id == local_node {
        if control.as_deref() != Some(requester_node_id) {
            return Err(Error::Denied(
                "replica writer snapshot may return only to its recorded control AwNode",
            ));
        }
        return Ok(());
    }
    if writer.writer_node_id != requester_node_id {
        return Err(Error::Denied(
            "Workspace snapshot requester is not the designated writer AwNode",
        ));
    }
    let latest:Option<(Option<String>,String,u64)>=db(c.query_row(
        "SELECT old_writer_node_id,new_writer_node_id,new_writer_epoch FROM workspace_writer_events WHERE workspace_id=?1 ORDER BY seq DESC LIMIT 1",
        [&workspace.workspace_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
    if !latest.is_some_and(|(old, new, epoch)| {
        old.as_deref() == Some(local_node.as_str())
            && new == requester_node_id
            && epoch == writer.writer_epoch
    }) {
        return Err(Error::Denied(
            "only the immediately fenced previous writer may serve the handoff snapshot",
        ));
    }
    if control.as_deref() == Some(requester_node_id) {
        // Imported replicas may return their final immutable/ref state to the
        // recorded control AwNode after that control node fences this writer.
        // No cross-host human identity equivalence is inferred here.
        return Ok(());
    }
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "initial Workspace writer handoff requires an active known user",
    ))?;
    let role:Option<String>=db(c.query_row(
        "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
        params![workspace.domain_id,user],|r|r.get(0)).optional())?;
    if !matches!(role.as_deref(), Some("owner" | "admin")) {
        return Err(Error::Denied(
            "initial Workspace writer handoff requires Domain owner/admin authority",
        ));
    }
    authority::require_domain_authority_write(c, &workspace.domain_id)
}

fn load_users(c: &Connection, ids: &BTreeSet<String>) -> Result<Vec<WorkspaceReplicaUser>> {
    let mut out = Vec::new();
    for id in ids {
        let row:(String,String,String,String,u64,i64)=db(c.query_row(
            "SELECT user_id,realm_id,status,display_json,security_epoch,created_at FROM users WHERE user_id=?1",
            [id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))))?;
        out.push(WorkspaceReplicaUser {
            user_id: row.0,
            realm_id: row.1,
            status: row.2,
            display: decode(&row.3)?,
            security_epoch: row.4,
            created_at: row.5,
        });
    }
    Ok(out)
}

fn load_writer_events(c: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceWriterEventInfo>> {
    let mut q=db(c.prepare("SELECT seq,event_id,workspace_id,domain_id,old_writer_node_id,old_writer_epoch,new_writer_node_id,new_writer_epoch,authority_term,actor,reason,created_at FROM workspace_writer_events WHERE workspace_id=?1 ORDER BY seq LIMIT ?2"))?;
    let rows = db(q.query_map(
        params![workspace_id, (MAX_REPLICA_WRITER_EVENTS + 1) as i64],
        |r| {
            Ok(WorkspaceWriterEventInfo {
                seq: r.get(0)?,
                event_id: r.get(1)?,
                workspace_id: r.get(2)?,
                domain_id: r.get(3)?,
                old_writer_node_id: r.get(4)?,
                old_writer_epoch: r.get(5)?,
                new_writer_node_id: r.get(6)?,
                new_writer_epoch: r.get(7)?,
                authority_term: r.get(8)?,
                actor: r.get(9)?,
                reason: r.get(10)?,
                created_at: r.get(11)?,
            })
        },
    ))?;
    let out = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if out.len() > MAX_REPLICA_WRITER_EVENTS {
        return Err(Error::Limit(
            "Workspace writer history exceeds Phase 29 handoff bound",
        ));
    }
    Ok(out)
}

fn load_branch_ref_events(
    c: &Connection,
    workspace_id: &str,
) -> Result<Vec<WorkspaceReplicaBranchRefEvent>> {
    let mut q=db(c.prepare("SELECT e.event_id,e.branch_id,e.old_head,e.old_revision,e.new_head,e.new_revision,e.actor,e.client_id,e.cause_type,e.cause_id,e.committed_at,e.writer_node_id,e.writer_epoch FROM branch_ref_events e JOIN branches b ON b.id=e.branch_id JOIN sessions s ON s.id=b.session_id WHERE s.workspace_id=?1 ORDER BY e.seq LIMIT ?2"))?;
    let rows = db(q.query_map(
        params![workspace_id, (MAX_REPLICA_REF_EVENTS + 1) as i64],
        |r| {
            Ok(WorkspaceReplicaBranchRefEvent {
                event_id: r.get(0)?,
                branch_id: r.get(1)?,
                old_head: r.get(2)?,
                old_revision: r.get(3)?,
                new_head: r.get(4)?,
                new_revision: r.get(5)?,
                actor: r.get(6)?,
                client_id: r.get(7)?,
                cause_type: r.get(8)?,
                cause_id: r.get(9)?,
                committed_at: r.get(10)?,
                writer_node_id: r.get(11)?,
                writer_epoch: r.get(12)?,
            })
        },
    ))?;
    let out = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if out.len() > MAX_REPLICA_REF_EVENTS {
        return Err(Error::Limit(
            "branch ref history exceeds Phase 29 handoff bound",
        ));
    }
    Ok(out)
}

fn load_replica_sessions(
    c: &Connection,
    workspace_id: &str,
) -> Result<(
    Vec<WorkspaceReplicaSession>,
    BTreeSet<String>,
    BTreeSet<String>,
)> {
    let mut q=db(c.prepare("SELECT id,owner,title,binding_json,root_key,root_node,next_turn,import_review_needed,owner_user_id,created_at,updated_at FROM sessions WHERE workspace_id=?1 ORDER BY created_at,id LIMIT ?2"))?;
    let rows = db(q.query_map(
        params![workspace_id, (MAX_REPLICA_SESSIONS + 1) as i64],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, u64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, i64>(9)?,
                r.get::<_, i64>(10)?,
            ))
        },
    ))?;
    let raw = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if raw.len() > MAX_REPLICA_SESSIONS {
        return Err(Error::Limit(
            "Workspace Session count exceeds Phase 29 handoff bound",
        ));
    }
    let mut user_ids = BTreeSet::new();
    let mut artifact_ids = BTreeSet::new();
    let mut out = Vec::new();
    for row in raw {
        let owner_user_id = row.8.ok_or(Error::Storage(
            "replicated Session is missing stable owner user ID",
        ))?;
        user_ids.insert(owner_user_id.clone());
        let mut mq=db(c.prepare("SELECT principal,can_append,principal_user_id FROM members WHERE session_id=?1 ORDER BY principal"))?;
        let members = db(db(mq.query_map([&row.0], |r| {
            Ok(WorkspaceReplicaMember {
                principal: r.get(0)?,
                can_append: r.get::<_, i64>(1)? != 0,
                principal_user_id: r.get(2)?,
            })
        }))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
        for m in &members {
            if let Some(id) = &m.principal_user_id {
                user_ids.insert(id.clone());
            }
        }
        let mut nq = db(c.prepare("SELECT id FROM nodes WHERE session_id=?1 ORDER BY seq"))?;
        let ids = db(db(nq.query_map([&row.0], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        if ids.len() > MAX_SESSION_NODES {
            return Err(Error::Limit("replicated Session exceeds node bound"));
        }
        let mut nodes = Vec::with_capacity(ids.len());
        for id in ids {
            let node = node_in(c, &row.0, &id)?;
            if matches!(node.kind.as_str(), "context_import" | "context_compaction") {
                if let Some(a) = node
                    .payload
                    .get("artifact_id")
                    .and_then(serde_json::Value::as_str)
                {
                    artifact_ids.insert(a.to_owned());
                }
            }
            nodes.push(WorkspaceReplicaNode {
                sha256: node_digest(&node)?,
                node,
            });
        }
        let mut bq=db(c.prepare("SELECT id,session_id,name,head,revision,active_run FROM branches WHERE session_id=?1 ORDER BY created_at,id"))?;
        let branches = db(db(bq.query_map([&row.0], |r| {
            Ok(BranchInfo {
                id: r.get(0)?,
                session_id: r.get(1)?,
                name: r.get(2)?,
                head: r.get(3)?,
                revision: r.get(4)?,
                active_run: r.get(5)?,
            })
        }))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
        if branches.iter().any(|b| b.active_run.is_some()) {
            return Err(Error::Conflict(
                "Workspace writer handoff cannot snapshot an active branch",
            ));
        }
        let mut aq=db(c.prepare("SELECT DISTINCT p.artifact_id FROM context_prompt_attachments p JOIN nodes n ON n.id=p.prompt_node WHERE n.session_id=?1 ORDER BY p.artifact_id"))?;
        for a in db(aq.query_map([&row.0], |r| r.get::<_, String>(0)))? {
            artifact_ids.insert(db(a)?);
        }
        out.push(WorkspaceReplicaSession {
            session_id: row.0,
            owner: row.1,
            title: row.2,
            binding: decode(&row.3)?,
            root_key: row.4,
            root_node: row.5,
            next_turn: row.6,
            import_review_needed: row.7 != 0,
            owner_user_id,
            created_at: row.9,
            updated_at: row.10,
            members,
            nodes,
            branches,
        });
    }
    Ok((out, user_ids, artifact_ids))
}

fn load_context(
    c: &Connection,
    workspace_id: &str,
    session_ids: &BTreeSet<String>,
    artifact_ids: &BTreeSet<String>,
) -> Result<(
    Vec<WorkspaceReplicaArtifact>,
    Vec<WorkspaceReplicaArtifactSource>,
    Vec<WorkspaceReplicaMaterializedFactLink>,
    Vec<WorkspaceReplicaNodeMaterializedFactLink>,
    Vec<WorkspaceReplicaNodeDependency>,
    Vec<WorkspaceReplicaDerivation>,
    Vec<WorkspaceReplicaPromptAttachment>,
    Vec<WorkspaceReplicaSourceGrant>,
    Vec<String>,
)> {
    let mut artifacts = Vec::new();
    let mut sources = Vec::new();
    let mut materialized_fact_links = Vec::new();
    for artifact_id in artifact_ids {
        let row: (String, String, i64, Option<String>) = db(c.query_row(
            "SELECT owner,body,created_at,owner_user_id FROM context_artifacts WHERE id=?1",
            [artifact_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ))?;
        let body: serde_json::Value = decode(&row.1)?;
        artifacts.push(WorkspaceReplicaArtifact {
            artifact_id: artifact_id.clone(),
            owner: row.0.clone(),
            owner_user_id: row.3,
            sha256: artifact_digest(artifact_id, &row.0, &body, row.2)?,
            body,
            created_at: row.2,
        });
        let mut q = db(c.prepare(
            "SELECT session_id,node_id,digest FROM context_artifact_sources WHERE artifact_id=?1 ORDER BY session_id,node_id",
        ))?;
        let rows = db(q.query_map([artifact_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }))?;
        for item in rows {
            let (session, node, digest) = db(item)?;
            if !session_ids.contains(&session) {
                return Err(Error::Denied(
                    "Workspace handoff refuses context artifacts that depend on Sessions outside this Workspace",
                ));
            }
            sources.push(WorkspaceReplicaArtifactSource {
                artifact_id: artifact_id.clone(),
                session_id: session,
                node_id: node,
                digest,
            });
        }
        let mut q = db(c.prepare(
            "SELECT materialization_id,ordinal,fact_json,fact_sha256,authorization_policy_id,authorization_policy_revision FROM context_artifact_materialized_facts WHERE artifact_id=?1 ORDER BY ordinal",
        ))?;
        let rows = db(q.query_map([artifact_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, u64>(5)?,
            ))
        }))?;
        for item in rows {
            let (
                materialization_id,
                ordinal,
                fact_json,
                fact_sha256,
                authorization_policy_id,
                authorization_policy_revision,
            ) = db(item)?;
            let fact: crate::durability::SemanticMaterializedFact = decode(&fact_json)?;
            if fact.id != materialization_id
                || crate::context::digest(&fact)? != fact_sha256
                || ordinal >= crate::context::materialized::MAX_MATERIALIZED_CONTEXT_FACTS as u64
            {
                return Err(Error::Conflict(
                    "Workspace handoff found invalid materialized-context provenance",
                ));
            }
            materialized_fact_links.push(WorkspaceReplicaMaterializedFactLink {
                artifact_id: artifact_id.clone(),
                materialization_id,
                ordinal,
                fact,
                fact_sha256,
                authorization_policy_id,
                authorization_policy_revision,
            });
        }
    }

    let mut node_materialized_fact_links = Vec::new();
    {
        let mut q = db(c.prepare(
            "SELECT m.node_id,m.materialization_id,m.fact_json,m.fact_sha256,m.authorization_policy_id,m.authorization_policy_revision FROM node_materialized_facts m JOIN nodes n ON n.id=m.node_id JOIN sessions s ON s.id=n.session_id WHERE s.workspace_id=?1 ORDER BY m.node_id,m.materialization_id",
        ))?;
        let rows = db(q.query_map([workspace_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, u64>(5)?,
            ))
        }))?;
        for item in rows {
            let (
                node_id,
                materialization_id,
                fact_json,
                fact_sha256,
                authorization_policy_id,
                authorization_policy_revision,
            ) = db(item)?;
            let fact: crate::durability::SemanticMaterializedFact = decode(&fact_json)?;
            if fact.id != materialization_id || crate::context::digest(&fact)? != fact_sha256 {
                return Err(Error::Conflict(
                    "Workspace handoff found invalid inherited materialized-context provenance",
                ));
            }
            node_materialized_fact_links.push(WorkspaceReplicaNodeMaterializedFactLink {
                node_id,
                materialization_id,
                fact,
                fact_sha256,
                authorization_policy_id,
                authorization_policy_revision,
            });
        }
    }

    let mut dependencies = Vec::new();
    {
        let mut q = db(c.prepare(
            "SELECT d.node_id,d.source_session,d.source_node FROM node_dependencies d JOIN nodes n ON n.id=d.node_id JOIN sessions s ON s.id=n.session_id WHERE s.workspace_id=?1 ORDER BY d.node_id,d.source_session,d.source_node",
        ))?;
        let rows = db(q.query_map([workspace_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }))?;
        for item in rows {
            let (node_id, source_session, source_node) = db(item)?;
            if !session_ids.contains(&source_session) {
                return Err(Error::Denied(
                    "Workspace handoff refuses node dependencies outside this Workspace",
                ));
            }
            dependencies.push(WorkspaceReplicaNodeDependency {
                node_id,
                source_session,
                source_node,
            });
        }
    }

    let mut derivations = Vec::new();
    {
        let mut q = db(c.prepare(
            "SELECT d.node_id,d.source_session,d.requester,d.grant_id,d.requester_user_id FROM context_derivations d JOIN nodes n ON n.id=d.node_id JOIN sessions s ON s.id=n.session_id WHERE s.workspace_id=?1 ORDER BY d.node_id,d.source_session,d.requester,d.grant_id",
        ))?;
        let rows = db(q.query_map([workspace_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        }))?;
        for item in rows {
            let (node_id, source_session, requester, grant_id, requester_user_id) = db(item)?;
            if !session_ids.contains(&source_session) {
                return Err(Error::Denied(
                    "Workspace handoff refuses derivations outside this Workspace",
                ));
            }
            derivations.push(WorkspaceReplicaDerivation {
                node_id,
                source_session,
                requester,
                requester_user_id,
                grant_id,
            });
        }
    }

    let mut prompt_attachments = Vec::new();
    {
        let mut q = db(c.prepare(
            "SELECT p.prompt_node,p.artifact_id FROM context_prompt_attachments p JOIN nodes n ON n.id=p.prompt_node JOIN sessions s ON s.id=n.session_id WHERE s.workspace_id=?1 ORDER BY p.prompt_node,p.artifact_id",
        ))?;
        let rows = db(q.query_map([workspace_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }))?;
        for item in rows {
            let (prompt_node, artifact_id) = db(item)?;
            prompt_attachments.push(WorkspaceReplicaPromptAttachment {
                prompt_node,
                artifact_id,
            });
        }
    }

    let mut source_grants = Vec::new();
    {
        let mut q = db(c.prepare(
            "SELECT g.session_id,g.principal,g.accounts_json,g.grant_id,g.principal_user_id FROM context_source_grants g JOIN sessions s ON s.id=g.session_id WHERE s.workspace_id=?1 ORDER BY g.session_id,g.principal",
        ))?;
        let rows = db(q.query_map([workspace_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        }))?;
        for item in rows {
            let (session_id, principal, accounts_json, grant_id, principal_user_id) = db(item)?;
            source_grants.push(WorkspaceReplicaSourceGrant {
                session_id,
                principal,
                principal_user_id,
                accounts: decode(&accounts_json)?,
                grant_id,
            });
        }
    }

    let private_sessions = {
        let mut q = db(c.prepare(
            "SELECT p.session_id FROM context_private_sessions p JOIN sessions s ON s.id=p.session_id WHERE s.workspace_id=?1 ORDER BY p.session_id",
        ))?;
        let rows = db(q.query_map([workspace_id], |r| r.get::<_, String>(0)))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())?
    };

    Ok((
        artifacts,
        sources,
        materialized_fact_links,
        node_materialized_fact_links,
        dependencies,
        derivations,
        prompt_attachments,
        source_grants,
        private_sessions,
    ))
}

fn validate_snapshot_shape(snapshot: &WorkspaceReplicaSnapshot) -> Result<()> {
    if snapshot.version != WORKSPACE_REPLICA_VERSION
        || snapshot.workspace.workspace_id != snapshot.writer.workspace_id
        || snapshot.workspace.domain_id != snapshot.domain.domain_id
        || snapshot.writer.domain_id != snapshot.domain.domain_id
        || snapshot.writer.writer_epoch == 0
        || snapshot.ref_writer_epoch > snapshot.writer.writer_epoch
        || snapshot.writer.state != "active"
        || snapshot.source_store_id.is_empty()
        || snapshot.source_node_id.is_empty()
    {
        return Err(Error::Protocol(
            "invalid Workspace replica snapshot envelope",
        ));
    }
    if snapshot_hash(snapshot)? != snapshot.snapshot_sha256 {
        return Err(Error::Conflict("Workspace replica snapshot hash mismatch"));
    }
    let mut sessions = BTreeSet::new();
    let mut nodes = BTreeSet::new();
    let mut branches = BTreeSet::new();
    for session in &snapshot.sessions {
        if !sessions.insert(session.session_id.clone()) {
            return Err(Error::Conflict(
                "duplicate Session ID in Workspace replica snapshot",
            ));
        }
        if session.nodes.is_empty() {
            return Err(Error::Protocol(
                "replicated Session has no immutable root graph",
            ));
        }
        for n in &session.nodes {
            if n.node.id.is_empty()
                || !nodes.insert(n.node.id.clone())
                || node_digest(&n.node)? != n.sha256
            {
                return Err(Error::Conflict(
                    "Workspace replica node identity/digest mismatch",
                ));
            }
        }
        if !session
            .nodes
            .iter()
            .any(|n| n.node.id == session.root_node && n.node.kind == "root")
        {
            return Err(Error::Protocol("replicated Session root is missing"));
        }
        for b in &session.branches {
            if b.session_id != session.session_id
                || b.active_run.is_some()
                || !branches.insert(b.id.clone())
                || !nodes.contains(&b.head)
            {
                return Err(Error::Protocol("invalid Workspace replica branch snapshot"));
            }
        }
    }
    let artifact_ids: BTreeSet<_> = snapshot
        .artifacts
        .iter()
        .map(|a| a.artifact_id.as_str())
        .collect();
    for artifact in &snapshot.artifacts {
        if artifact_digest(
            &artifact.artifact_id,
            &artifact.owner,
            &artifact.body,
            artifact.created_at,
        )? != artifact.sha256
        {
            return Err(Error::Conflict(
                "Workspace replica context artifact digest mismatch",
            ));
        }
    }
    let mut fact_links = BTreeSet::new();
    let mut fact_ordinals = BTreeSet::new();
    for link in &snapshot.materialized_fact_links {
        if !artifact_ids.contains(link.artifact_id.as_str())
            || link.fact.id != link.materialization_id
            || crate::context::digest(&link.fact)? != link.fact_sha256
            || link.ordinal >= crate::context::materialized::MAX_MATERIALIZED_CONTEXT_FACTS as u64
            || link.authorization_policy_id.is_empty()
            || link.authorization_policy_revision == 0
            || !fact_links.insert((link.artifact_id.clone(), link.materialization_id.clone()))
            || !fact_ordinals.insert((link.artifact_id.clone(), link.ordinal))
        {
            return Err(Error::Conflict(
                "Workspace replica materialized-context provenance mismatch",
            ));
        }
    }
    let mut node_fact_links = BTreeSet::new();
    for link in &snapshot.node_materialized_fact_links {
        if !nodes.contains(&link.node_id)
            || link.fact.id != link.materialization_id
            || crate::context::digest(&link.fact)? != link.fact_sha256
            || link.authorization_policy_id.is_empty()
            || link.authorization_policy_revision == 0
            || !node_fact_links.insert((link.node_id.clone(), link.materialization_id.clone()))
        {
            return Err(Error::Conflict(
                "Workspace replica inherited materialized-context provenance mismatch",
            ));
        }
    }
    let last = snapshot.writer_events.last().ok_or(Error::Protocol(
        "Workspace replica snapshot has no writer provenance",
    ))?;
    if last.new_writer_epoch != snapshot.writer.writer_epoch
        || last.new_writer_node_id != snapshot.writer.writer_node_id
        || last.authority_term != snapshot.writer.authority_term
    {
        return Err(Error::Conflict(
            "Workspace replica writer state does not match immutable writer history",
        ));
    }
    Ok(())
}

fn safe_user_upsert(c: &Connection, user: &WorkspaceReplicaUser) -> Result<()> {
    let existing:Option<(String,String,String,u64,i64)>=db(c.query_row("SELECT realm_id,status,display_json,security_epoch,created_at FROM users WHERE user_id=?1",[&user.user_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional())?;
    let display = json(&user.display)?;
    if let Some(old) = existing {
        if old
            != (
                user.realm_id.clone(),
                user.status.clone(),
                display,
                user.security_epoch,
                user.created_at,
            )
        {
            return Err(Error::Conflict("stable User ID collides with different safe metadata during Workspace replica import"));
        }
    } else {
        db(c.execute("INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![user.user_id,user.realm_id,user.status,display,user.security_epoch,user.created_at]))?;
    }
    Ok(())
}

fn insert_or_verify_writer_event(c: &Connection, event: &WorkspaceWriterEventInfo) -> Result<()> {
    let changed=db(c.execute("INSERT OR IGNORE INTO workspace_writer_events(event_id,workspace_id,domain_id,old_writer_node_id,old_writer_epoch,new_writer_node_id,new_writer_epoch,authority_term,actor,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![event.event_id,event.workspace_id,event.domain_id,event.old_writer_node_id,event.old_writer_epoch,event.new_writer_node_id,event.new_writer_epoch,event.authority_term,event.actor,event.reason,event.created_at]))?;
    if changed == 0 {
        let same:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM workspace_writer_events WHERE event_id=?1 AND workspace_id=?2 AND domain_id=?3 AND old_writer_node_id IS ?4 AND old_writer_epoch IS ?5 AND new_writer_node_id=?6 AND new_writer_epoch=?7 AND authority_term=?8 AND actor=?9 AND reason=?10 AND created_at=?11)",params![event.event_id,event.workspace_id,event.domain_id,event.old_writer_node_id,event.old_writer_epoch,event.new_writer_node_id,event.new_writer_epoch,event.authority_term,event.actor,event.reason,event.created_at],|r|r.get(0)))?;
        if !same {
            return Err(Error::Conflict(
                "Workspace writer event ID collision during replica import",
            ));
        }
    }
    Ok(())
}

fn insert_or_verify_ref_event(
    c: &Connection,
    event: &WorkspaceReplicaBranchRefEvent,
) -> Result<()> {
    let changed=db(c.execute("INSERT OR IGNORE INTO branch_ref_events(event_id,branch_id,old_head,old_revision,new_head,new_revision,actor,client_id,cause_type,cause_id,committed_at,writer_node_id,writer_epoch) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![event.event_id,event.branch_id,event.old_head,event.old_revision,event.new_head,event.new_revision,event.actor,event.client_id,event.cause_type,event.cause_id,event.committed_at,event.writer_node_id,event.writer_epoch]))?;
    if changed == 0 {
        let same:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM branch_ref_events WHERE event_id=?1 AND branch_id=?2 AND old_head IS ?3 AND old_revision IS ?4 AND new_head=?5 AND new_revision=?6 AND actor=?7 AND client_id IS ?8 AND cause_type=?9 AND cause_id IS ?10 AND committed_at=?11 AND writer_node_id IS ?12 AND writer_epoch=?13)",params![event.event_id,event.branch_id,event.old_head,event.old_revision,event.new_head,event.new_revision,event.actor,event.client_id,event.cause_type,event.cause_id,event.committed_at,event.writer_node_id,event.writer_epoch],|r|r.get(0)))?;
        if !same {
            return Err(Error::Conflict(
                "branch ref event ID collision during Workspace replica import",
            ));
        }
    }
    Ok(())
}

fn is_ancestor(c: &Connection, session: &str, ancestor: &str, descendant: &str) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }
    let mut current = Some(descendant.to_owned());
    for _ in 0..=MAX_SESSION_NODES {
        let Some(id) = current else { return Ok(false) };
        if id == ancestor {
            return Ok(true);
        }
        current = db(c
            .query_row(
                "SELECT parent_id FROM nodes WHERE session_id=?1 AND id=?2",
                params![session, id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional())?
        .flatten();
    }
    Err(Error::Limit(
        "Workspace replica ancestry exceeds node bound",
    ))
}

fn preserve_divergence(
    c: &Connection,
    actor: &PrincipalId,
    workspace_id: &str,
    session_id: &str,
    canonical: &BranchInfo,
    incoming: &BranchInfo,
    local_epoch: u64,
    incoming_epoch: u64,
    preserve_incoming: bool,
    txid: &str,
) -> Result<bool> {
    let key = digest(
        &serde_json::json!({"workspace":workspace_id,"session":session_id,"branch":canonical.id,"local_epoch":local_epoch,"local_head":canonical.head,"incoming_epoch":incoming_epoch,"incoming_head":incoming.head}),
    )?;
    let divergence_id = format!("wdiv_{}", &key[..32]);
    let preserved_id = format!("bconf_{}", &key[..32]);
    let (preserved_head, preserved_revision, preserved_epoch) = if preserve_incoming {
        (&incoming.head, incoming.revision, incoming_epoch)
    } else {
        (&canonical.head, canonical.revision, local_epoch)
    };
    let name = format!("diverged-e{}-{}", preserved_epoch, &key[..12]);
    let exists: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspace_replica_divergences WHERE divergence_id=?1)",
        [&divergence_id],
        |r| r.get(0),
    ))?;
    if exists {
        return Ok(false);
    }
    let branch_exists: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM branches WHERE id=?1)",
        [&preserved_id],
        |r| r.get(0),
    ))?;
    if !branch_exists {
        db(c.execute("INSERT INTO branches(id,session_id,name,head,revision,active_run,created_at) VALUES(?1,?2,?3,?4,?5,NULL,?6)",params![preserved_id,session_id,name,preserved_head,preserved_revision,now()]))?;
        let created = branch_in(c, session_id, &preserved_id)?;
        durability::ref_event(
            c,
            &BranchInfo {
                id: created.id.clone(),
                session_id: created.session_id.clone(),
                name: created.name.clone(),
                head: preserved_head.clone(),
                revision: preserved_revision,
                active_run: None,
            },
            preserved_head,
            preserved_revision,
            actor,
            "replica_divergence_preserved",
            Some(&divergence_id),
            txid,
        )?;
    }
    db(c.execute("INSERT INTO workspace_replica_divergences(divergence_id,workspace_id,session_id,canonical_branch_id,preserved_branch_id,local_writer_epoch,incoming_writer_epoch,local_head,incoming_head,detected_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![divergence_id,workspace_id,session_id,canonical.id,preserved_id,local_epoch,incoming_epoch,canonical.head,incoming.head,now()]))?;
    Ok(true)
}

impl SessionStore {
    pub fn export_workspace_replica_snapshot(
        &self,
        actor: &PrincipalId,
        workspace_id: &str,
        requester_node_id: &str,
    ) -> Result<WorkspaceReplicaSnapshot> {
        let peer = self.awnet_peer_by_node_id(requester_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied(
                "Workspace handoff requester peer is not trusted",
            ));
        }
        let c = self.connection()?;
        let workspace = workspace_row_raw(&c, workspace_id)?;
        let writer = workspace_writer::writer_row(&c, workspace_id)?;
        require_export_authority(&c, actor, &workspace, &writer, requester_node_id)?;
        require_quiescent(&c, workspace_id)?;
        let domain = domain_row_raw(&c, &workspace.domain_id)?;
        let source_store_id: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))?;
        let source_node_id: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        let (sessions, mut user_ids, artifact_ids) = load_replica_sessions(&c, workspace_id)?;
        if let Some(id) = &workspace.created_by_user_id {
            user_ids.insert(id.clone());
        }
        let session_ids: BTreeSet<String> = sessions.iter().map(|s| s.session_id.clone()).collect();
        let (
            artifacts,
            artifact_sources,
            materialized_fact_links,
            node_materialized_fact_links,
            node_dependencies,
            derivations,
            prompt_attachments,
            source_grants,
            private_sessions,
        ) = load_context(&c, workspace_id, &session_ids, &artifact_ids)?;
        let users = load_users(&c, &user_ids)?;
        let writer_events = load_writer_events(&c, workspace_id)?;
        let branch_ref_events = load_branch_ref_events(&c, workspace_id)?;
        let ref_writer_epoch = branch_ref_events
            .iter()
            .map(|event| event.writer_epoch)
            .max()
            .unwrap_or(0);
        let mut snapshot = WorkspaceReplicaSnapshot {
            version: WORKSPACE_REPLICA_VERSION,
            source_store_id,
            source_node_id,
            domain,
            workspace,
            writer,
            ref_writer_epoch,
            writer_events,
            users,
            sessions,
            artifacts,
            artifact_sources,
            materialized_fact_links,
            node_materialized_fact_links,
            node_dependencies,
            derivations,
            prompt_attachments,
            source_grants,
            private_sessions,
            branch_ref_events,
            created_at: now(),
            snapshot_sha256: String::new(),
        };
        snapshot.snapshot_sha256 = snapshot_hash(&snapshot)?;
        if json(&snapshot)?.len() > MAX_REPLICA_BYTES {
            return Err(Error::Limit(
                "Workspace replica snapshot exceeds Phase 29 3 MiB handoff bound",
            ));
        }
        validate_snapshot_shape(&snapshot)?;
        Ok(snapshot)
    }

    pub fn import_workspace_replica_snapshot(
        &self,
        actor: &PrincipalId,
        authenticated_source_node: &str,
        snapshot: &WorkspaceReplicaSnapshot,
        source_signature: &str,
    ) -> Result<WorkspaceReplicaApplyResult> {
        validate_snapshot_shape(snapshot)?;
        if snapshot.source_node_id != authenticated_source_node {
            return Err(Error::Denied(
                "Workspace replica source is not the authenticated MachineLink peer",
            ));
        }
        if json(snapshot)?.len() > MAX_REPLICA_BYTES {
            return Err(Error::Limit(
                "Workspace replica snapshot exceeds Phase 29 handoff bound",
            ));
        }
        let peer = self.awnet_peer_by_node_id(authenticated_source_node)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied(
                "Workspace replica source peer is not trusted",
            ));
        }
        verify_snapshot_signature(&peer.node_id, &peer.public_key, snapshot, source_signature)?;
        let local = self.local_machine()?;
        let local_store = self.store_id()?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let existing_snapshot:Option<String>=db(tx.query_row("SELECT snapshot_id FROM workspace_replica_snapshots WHERE source_store_id=?1 AND workspace_id=?2 AND snapshot_sha256=?3",params![snapshot.source_store_id,snapshot.workspace.workspace_id,snapshot.snapshot_sha256],|r|r.get(0)).optional())?;
        if let Some(id) = existing_snapshot {
            let writer = workspace_writer::writer_row(&tx, &snapshot.workspace.workspace_id)?;
            drop(tx);
            return Ok(WorkspaceReplicaApplyResult {
                snapshot: snapshot_info_row(&c, &id)?,
                writer,
                sessions_imported: 0,
                nodes_imported: 0,
                divergences_preserved: 0,
                reused: true,
            });
        }
        let domain_existing: Option<(String, String)> = db(tx
            .query_row(
                "SELECT authority_store_id,name FROM domains WHERE domain_id=?1",
                [&snapshot.domain.domain_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        if let Some((authority_store, name)) = &domain_existing {
            if name != &snapshot.domain.name {
                return Err(Error::Conflict(
                    "Workspace replica Domain ID collides with another local Domain",
                ));
            }
            if authority_store != &snapshot.domain.authority_store_id
                && authority_store != &local_store
            {
                return Err(Error::Conflict(
                    "Workspace replica Domain authority store conflicts with local metadata",
                ));
            }
        } else {
            if let Some(parent) = snapshot.domain.parent_domain_id.as_deref() {
                let parent_exists: bool = db(tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM domains WHERE domain_id=?1)",
                    [parent],
                    |r| r.get(0),
                ))?;
                if !parent_exists {
                    return Err(Error::Denied("Workspace replica child Domain requires its parent Domain to exist locally first"));
                }
            }
            db(tx.execute("INSERT INTO domains(domain_id,authority_store_id,parent_domain_id,name,status,policy_revision,security_epoch,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![snapshot.domain.domain_id,snapshot.domain.authority_store_id,snapshot.domain.parent_domain_id,snapshot.domain.name,snapshot.domain.status,snapshot.domain.policy_revision,snapshot.domain.security_epoch,snapshot.domain.created_at,snapshot.domain.updated_at]))?;
            db(tx.execute("INSERT INTO workspace_replica_domains(domain_id,source_authority_store_id,source_node_id,created_at) VALUES(?1,?2,?3,?4)",params![snapshot.domain.domain_id,snapshot.domain.authority_store_id,authenticated_source_node,now()]))?;
        }
        for user in &snapshot.users {
            safe_user_upsert(&tx, user)?;
        }
        let existing_workspace: Option<(String, String)> = db(tx
            .query_row(
                "SELECT domain_id,name FROM workspaces WHERE workspace_id=?1",
                [&snapshot.workspace.workspace_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        let existing_replica_origin: Option<(String,String)> = db(tx.query_row(
            "SELECT source_authority_store_id,source_workspace_id FROM workspace_replica_origins WHERE workspace_id=?1",
            [&snapshot.workspace.workspace_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional())?;
        let had_replica_origin = existing_replica_origin.is_some();
        let mut local_writer_before = None;
        if let Some((domain, name)) = &existing_workspace {
            if domain != &snapshot.workspace.domain_id || name != &snapshot.workspace.name {
                return Err(Error::Conflict(
                    "Workspace replica ID collides with another local Workspace",
                ));
            }
            if let Some((store, source_ws)) = existing_replica_origin.clone() {
                if store != snapshot.domain.authority_store_id
                    || source_ws != snapshot.workspace.workspace_id
                {
                    return Err(Error::Conflict("Workspace replica origin binding changed"));
                }
            }
            local_writer_before = Some(workspace_writer::writer_row(
                &tx,
                &snapshot.workspace.workspace_id,
            )?);
        } else {
            if snapshot.writer.writer_node_id != local.node_id {
                return Err(Error::Denied(
                    "new Workspace replica can activate only on its designated writer AwNode",
                ));
            }
            db(tx.execute("INSERT INTO workspace_replica_import_intents(workspace_id,source_node_id,created_at) VALUES(?1,?2,?3)",params![snapshot.workspace.workspace_id,authenticated_source_node,now()]))?;
            db(tx.execute("INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,host_machine_id,policy_json,defaults_json,policy_revision,revision,created_by_user_id,created_at,updated_at,archived_at) VALUES(?1,?2,?3,'hosted','peer_created',?4,?5,?6,?7,?8,?9,?10,?11,NULL)",params![snapshot.workspace.workspace_id,snapshot.workspace.domain_id,snapshot.workspace.name,local.id,json(&snapshot.workspace.policy)?,json(&snapshot.workspace.defaults)?,snapshot.workspace.policy_revision,snapshot.workspace.revision,snapshot.workspace.created_by_user_id,snapshot.workspace.created_at,snapshot.workspace.updated_at]))?;
            db(tx.execute("INSERT INTO workspace_writer_state(workspace_id,domain_id,writer_node_id,writer_epoch,authority_term,state,revision,updated_at) VALUES(?1,?2,?3,?4,?5,'active',?6,?7)",params![snapshot.workspace.workspace_id,snapshot.workspace.domain_id,snapshot.writer.writer_node_id,snapshot.writer.writer_epoch,snapshot.writer.authority_term,snapshot.writer.revision,snapshot.writer.updated_at]))?;
            db(tx.execute("INSERT INTO workspace_replica_origins(workspace_id,source_domain_id,source_workspace_id,source_authority_store_id,control_node_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",params![snapshot.workspace.workspace_id,snapshot.domain.domain_id,snapshot.workspace.workspace_id,snapshot.domain.authority_store_id,authenticated_source_node,now()]))?;
        }
        let current_writer = workspace_writer::writer_row(&tx, &snapshot.workspace.workspace_id)?;
        let current_epoch_mutated: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM branch_ref_events e JOIN branches b ON b.id=e.branch_id JOIN sessions s ON s.id=b.session_id WHERE s.workspace_id=?1 AND e.writer_epoch=?2 AND e.writer_node_id=?3)",
            params![snapshot.workspace.workspace_id,current_writer.writer_epoch,current_writer.writer_node_id],
            |r| r.get(0),
        ))?;
        let activation_mode = if existing_workspace.is_none() {
            "new"
        } else if snapshot.writer.writer_epoch > current_writer.writer_epoch
            && snapshot.writer.writer_node_id == local.node_id
        {
            "advance"
        } else if snapshot.writer.writer_epoch == current_writer.writer_epoch
            && snapshot.writer.writer_node_id == current_writer.writer_node_id
            && snapshot.writer.authority_term == current_writer.authority_term
            && snapshot.source_node_id != current_writer.writer_node_id
            && snapshot.ref_writer_epoch.saturating_add(1) == current_writer.writer_epoch
        {
            if current_epoch_mutated {
                "fenced_previous_writer_diverged"
            } else {
                "fenced_previous_writer_refresh"
            }
        } else if snapshot.writer.writer_epoch == current_writer.writer_epoch
            && snapshot.writer.writer_node_id == current_writer.writer_node_id
            && snapshot.writer.authority_term == current_writer.authority_term
        {
            "same"
        } else if current_writer.writer_node_id == local.node_id
            && current_writer.writer_epoch == snapshot.writer.writer_epoch.saturating_add(1)
        {
            if current_epoch_mutated {
                "previous_writer_diverged"
            } else {
                "previous_writer_refresh"
            }
        } else {
            return Err(Error::Denied("Workspace replica snapshot is stale or not bound to this AwNode's current writer handoff"));
        };
        if activation_mode == "advance" {
            db(tx.execute("UPDATE workspace_writer_state SET writer_node_id=?1,writer_epoch=?2,authority_term=?3,state='active',revision=MAX(revision+1,?4),updated_at=?5 WHERE workspace_id=?6",params![snapshot.writer.writer_node_id,snapshot.writer.writer_epoch,snapshot.writer.authority_term,snapshot.writer.revision,now(),snapshot.workspace.workspace_id]))?;
        }
        for event in &snapshot.writer_events {
            insert_or_verify_writer_event(&tx, event)?;
        }
        let mut sessions_imported = 0usize;
        let mut nodes_imported = 0usize;
        for session in &snapshot.sessions {
            let existing:Option<(String,String,String,String,String,Option<String>,i64)>=db(tx.query_row("SELECT owner,title,binding_json,root_key,root_node,owner_user_id,created_at FROM sessions WHERE id=?1",[&session.session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional())?;
            let binding = json(&session.binding)?;
            if let Some(old) = existing {
                if old
                    != (
                        session.owner.clone(),
                        session.title.clone(),
                        binding.clone(),
                        session.root_key.clone(),
                        session.root_node.clone(),
                        Some(session.owner_user_id.clone()),
                        session.created_at,
                    )
                {
                    return Err(Error::Conflict(
                        "replicated Session ID collides with different immutable metadata",
                    ));
                }
                db(tx.execute("UPDATE sessions SET next_turn=MAX(next_turn,?1),updated_at=MAX(updated_at,?2),import_review_needed=MAX(import_review_needed,?3) WHERE id=?4",params![session.next_turn,session.updated_at,if session.import_review_needed{1}else{0},session.session_id]))?;
            } else {
                db(tx.execute("INSERT INTO sessions(id,owner,title,binding_json,root_key,root_node,next_turn,import_review_needed,created_at,updated_at,owner_user_id,workspace_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![session.session_id,session.owner,session.title,binding,session.root_key,session.root_node,session.next_turn,if session.import_review_needed{1}else{0},session.created_at,session.updated_at,session.owner_user_id,snapshot.workspace.workspace_id]))?;
                sessions_imported += 1;
            }
            db(tx.execute(
                "DELETE FROM members WHERE session_id=?1",
                [&session.session_id],
            ))?;
            for member in &session.members {
                db(tx.execute("INSERT INTO members(session_id,principal,can_append,principal_user_id) VALUES(?1,?2,?3,?4)",params![session.session_id,member.principal,if member.can_append{1}else{0},member.principal_user_id]))?;
            }
            for object in &session.nodes {
                let exists: Option<(Option<String>, String, String, String, i64)> = db(tx
                    .query_row(
                        "SELECT parent_id,kind,actor,payload,created_at FROM nodes WHERE id=?1",
                        [&object.node.id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    )
                    .optional())?;
                let payload = json(&object.node.payload)?;
                if let Some(old) = exists {
                    if old
                        != (
                            object.node.parent_id.clone(),
                            object.node.kind.clone(),
                            object.node.actor.as_str().to_owned(),
                            payload,
                            object.node.created_at,
                        )
                        || node_digest(&object.node)? != object.sha256
                    {
                        return Err(Error::Conflict(
                            "immutable node ID collision during Workspace replica import",
                        ));
                    }
                } else {
                    db(tx.execute("INSERT INTO nodes(id,session_id,parent_id,kind,actor,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![object.node.id,session.session_id,object.node.parent_id,object.node.kind,object.node.actor.as_str(),payload,object.node.created_at]))?;
                    nodes_imported += 1;
                }
            }
        }
        for artifact in &snapshot.artifacts {
            let body = json(&artifact.body)?;
            let existing: Option<(String, String, i64, Option<String>)> = db(tx
                .query_row(
                    "SELECT owner,body,created_at,owner_user_id FROM context_artifacts WHERE id=?1",
                    [&artifact.artifact_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional())?;
            if let Some(old) = existing {
                if (old.0, old.1, old.2)
                    != (artifact.owner.clone(), body.clone(), artifact.created_at)
                    || old
                        .3
                        .as_ref()
                        .zip(artifact.owner_user_id.as_ref())
                        .is_some_and(|(a, b)| a != b)
                    || artifact_digest(
                        &artifact.artifact_id,
                        &artifact.owner,
                        &artifact.body,
                        artifact.created_at,
                    )? != artifact.sha256
                {
                    return Err(Error::Conflict(
                        "context artifact ID collision during Workspace replica import",
                    ));
                }
                if old.3.is_none() && artifact.owner_user_id.is_some() {
                    db(tx.execute(
                        "UPDATE context_artifacts SET owner_user_id=?1 WHERE id=?2 AND owner_user_id IS NULL",
                        params![artifact.owner_user_id,artifact.artifact_id],
                    ))?;
                }
            } else {
                db(tx.execute("INSERT INTO context_artifacts(id,owner,plan_id,operation_id,body,created_at,owner_user_id) VALUES(?1,?2,NULL,NULL,?3,?4,?5)",params![artifact.artifact_id,artifact.owner,body,artifact.created_at,artifact.owner_user_id]))?;
            }
        }
        for row in &snapshot.artifact_sources {
            db(tx.execute("INSERT OR IGNORE INTO context_artifact_sources(artifact_id,session_id,node_id,digest) VALUES(?1,?2,?3,?4)",params![row.artifact_id,row.session_id,row.node_id,row.digest]))?;
        }
        for row in &snapshot.materialized_fact_links {
            let body = json(&row.fact)?;
            let changed = db(tx.execute(
                "INSERT OR IGNORE INTO context_artifact_materialized_facts(artifact_id,materialization_id,ordinal,fact_json,fact_sha256,authorization_policy_id,authorization_policy_revision) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![row.artifact_id,row.materialization_id,row.ordinal,body,row.fact_sha256,row.authorization_policy_id,row.authorization_policy_revision],
            ))?;
            if changed == 0 {
                let same: bool = db(tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM context_artifact_materialized_facts WHERE artifact_id=?1 AND materialization_id=?2 AND ordinal=?3 AND fact_json=?4 AND fact_sha256=?5 AND authorization_policy_id=?6 AND authorization_policy_revision=?7)",
                    params![row.artifact_id,row.materialization_id,row.ordinal,body,row.fact_sha256,row.authorization_policy_id,row.authorization_policy_revision],
                    |r| r.get(0),
                ))?;
                if !same {
                    return Err(Error::Conflict(
                        "materialized-context provenance collision during Workspace replica import",
                    ));
                }
            }
        }
        for row in &snapshot.node_materialized_fact_links {
            let body = json(&row.fact)?;
            let changed = db(tx.execute(
                "INSERT OR IGNORE INTO node_materialized_facts(node_id,materialization_id,fact_json,fact_sha256,authorization_policy_id,authorization_policy_revision) VALUES(?1,?2,?3,?4,?5,?6)",
                params![row.node_id,row.materialization_id,body,row.fact_sha256,row.authorization_policy_id,row.authorization_policy_revision],
            ))?;
            if changed == 0 {
                let same: bool = db(tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM node_materialized_facts WHERE node_id=?1 AND materialization_id=?2 AND fact_json=?3 AND fact_sha256=?4 AND authorization_policy_id=?5 AND authorization_policy_revision=?6)",
                    params![row.node_id,row.materialization_id,body,row.fact_sha256,row.authorization_policy_id,row.authorization_policy_revision],
                    |r| r.get(0),
                ))?;
                if !same {
                    return Err(Error::Conflict(
                        "inherited materialized-context provenance collision during Workspace replica import",
                    ));
                }
            }
        }
        for row in &snapshot.node_dependencies {
            db(tx.execute("INSERT OR IGNORE INTO node_dependencies(node_id,source_session,source_node) VALUES(?1,?2,?3)",params![row.node_id,row.source_session,row.source_node]))?;
        }
        for row in &snapshot.derivations {
            db(tx.execute(
                "INSERT INTO context_derivations(node_id,source_session,requester,grant_id,requester_user_id) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(node_id,source_session,requester,grant_id) DO UPDATE SET requester_user_id=excluded.requester_user_id WHERE context_derivations.requester_user_id IS NULL AND excluded.requester_user_id IS NOT NULL",
                params![row.node_id,row.source_session,row.requester,row.grant_id,row.requester_user_id],
            ))?;
        }
        for row in &snapshot.prompt_attachments {
            db(tx.execute("INSERT OR IGNORE INTO context_prompt_attachments(prompt_node,artifact_id) VALUES(?1,?2)",params![row.prompt_node,row.artifact_id]))?;
        }
        for session in &snapshot.sessions {
            db(tx.execute(
                "DELETE FROM context_source_grants WHERE session_id=?1",
                [&session.session_id],
            ))?;
        }
        for row in &snapshot.source_grants {
            db(tx.execute("INSERT INTO context_source_grants(session_id,principal,accounts_json,grant_id,principal_user_id) VALUES(?1,?2,?3,?4,?5)",params![row.session_id,row.principal,json(&row.accounts)?,row.grant_id,row.principal_user_id]))?;
        }
        for session in &snapshot.private_sessions {
            db(tx.execute(
                "INSERT OR IGNORE INTO context_private_sessions(session_id) VALUES(?1)",
                [session],
            ))?;
        }
        let writer_after = workspace_writer::writer_row(&tx, &snapshot.workspace.workspace_id)?;
        let txid = durability::tx_id()?;
        let mut divergences = 0usize;
        for session in &snapshot.sessions {
            for incoming in &session.branches {
                let existing:Option<BranchInfo>=db(tx.query_row("SELECT id,session_id,name,head,revision,active_run FROM branches WHERE id=?1",[&incoming.id],|r|Ok(BranchInfo{id:r.get(0)?,session_id:r.get(1)?,name:r.get(2)?,head:r.get(3)?,revision:r.get(4)?,active_run:r.get(5)?})).optional())?;
                if let Some(current) = existing {
                    if current.session_id != incoming.session_id
                        || current.name != incoming.name
                        || current.active_run.is_some()
                    {
                        return Err(Error::Conflict(
                            "replicated canonical branch collides with incompatible local branch",
                        ));
                    }
                    if current.head == incoming.head && current.revision == incoming.revision {
                        continue;
                    }
                    let incoming_is_ancestor =
                        is_ancestor(&tx, &session.session_id, &incoming.head, &current.head)?;
                    let local_is_ancestor =
                        is_ancestor(&tx, &session.session_id, &current.head, &incoming.head)?;
                    let local_epoch = local_writer_before
                        .as_ref()
                        .map_or(writer_after.writer_epoch, |w| w.writer_epoch);

                    // Stale/non-writer snapshots may contribute immutable descendants,
                    // but they never roll back the current canonical ref. Preserve the
                    // incoming descendant explicitly when needed.
                    let keep_local_canonical = matches!(
                        activation_mode,
                        "previous_writer_diverged" | "fenced_previous_writer_diverged"
                    ) || (activation_mode == "same"
                        && snapshot.source_node_id != current_writer.writer_node_id);
                    if keep_local_canonical {
                        if incoming_is_ancestor {
                            continue;
                        }
                        if preserve_divergence(
                            &tx,
                            actor,
                            &snapshot.workspace.workspace_id,
                            &session.session_id,
                            &current,
                            incoming,
                            local_epoch,
                            snapshot.ref_writer_epoch,
                            true,
                            &txid,
                        )? {
                            divergences += 1;
                        }
                        continue;
                    }

                    // A newly authoritative incoming writer becomes canonical. If the
                    // prior local ref diverged, preserve that immutable descendant first.
                    if !local_is_ancestor
                        && preserve_divergence(
                            &tx,
                            actor,
                            &snapshot.workspace.workspace_id,
                            &session.session_id,
                            &current,
                            incoming,
                            local_epoch,
                            snapshot.ref_writer_epoch,
                            false,
                            &txid,
                        )?
                    {
                        divergences += 1;
                    }
                    let next_revision = current.revision.saturating_add(1).max(incoming.revision);
                    let changed=db(tx.execute("UPDATE branches SET head=?1,revision=?2,active_run=NULL WHERE id=?3 AND head=?4 AND revision=?5 AND active_run IS NULL",params![incoming.head,next_revision,current.id,current.head,current.revision]))?;
                    if changed != 1 {
                        return Err(Error::Conflict(
                            "canonical branch changed during Workspace replica handoff",
                        ));
                    }
                    durability::ref_event(
                        &tx,
                        &current,
                        &incoming.head,
                        next_revision,
                        actor,
                        "workspace_replica_handoff",
                        Some(&snapshot.snapshot_sha256),
                        &txid,
                    )?;
                } else {
                    db(tx.execute("INSERT INTO branches(id,session_id,name,head,revision,active_run,created_at) VALUES(?1,?2,?3,?4,?5,NULL,?6)",params![incoming.id,incoming.session_id,incoming.name,incoming.head,incoming.revision,now()]))?;
                }
            }
        }
        for event in &snapshot.branch_ref_events {
            insert_or_verify_ref_event(&tx, event)?;
        }
        for session in &snapshot.sessions {
            for branch in &session.branches {
                let s = session_in(&tx, &session.session_id)?;
                let b = branch_in(&tx, &session.session_id, &branch.id)?;
                let _ = history_in(&tx, &s, &b)?;
            }
        }
        if existing_workspace.is_none() {
            db(tx.execute(
                "DELETE FROM workspace_replica_import_intents WHERE workspace_id=?1",
                [&snapshot.workspace.workspace_id],
            ))?;
        }
        if existing_workspace.is_none() || had_replica_origin {
            db(tx.execute("INSERT INTO workspace_replica_origins(workspace_id,source_domain_id,source_workspace_id,source_authority_store_id,control_node_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(workspace_id) DO UPDATE SET control_node_id=excluded.control_node_id,updated_at=excluded.updated_at",params![snapshot.workspace.workspace_id,snapshot.domain.domain_id,snapshot.workspace.workspace_id,snapshot.domain.authority_store_id,authenticated_source_node,now()]))?;
        }
        let snapshot_id = format!("wrsnap_{}", &snapshot.snapshot_sha256[..32]);
        db(tx.execute("INSERT INTO workspace_replica_snapshots(snapshot_id,workspace_id,source_store_id,source_node_id,writer_epoch,authority_term,snapshot_sha256,body_json,source_signature,imported_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![snapshot_id,snapshot.workspace.workspace_id,snapshot.source_store_id,snapshot.source_node_id,snapshot.writer.writer_epoch,snapshot.writer.authority_term,snapshot.snapshot_sha256,json(snapshot)?,source_signature,now()]))?;
        durability::commit_event(
            &tx,
            &txid,
            "workspace.replica_handoff_applied",
            "workspace",
            &snapshot.workspace.workspace_id,
            Some(actor),
            serde_json::json!({"source_store_id":snapshot.source_store_id,"source_node_id":snapshot.source_node_id,"writer_epoch":writer_after.writer_epoch,"authority_term":writer_after.authority_term,"snapshot_sha256":snapshot.snapshot_sha256,"divergences_preserved":divergences}),
        )?;
        let writer = workspace_writer::writer_row(&tx, &snapshot.workspace.workspace_id)?;
        db(tx.commit())?;
        Ok(WorkspaceReplicaApplyResult {
            snapshot: snapshot_info_row(&c, &snapshot_id)?,
            writer,
            sessions_imported,
            nodes_imported,
            divergences_preserved: divergences,
            reused: false,
        })
    }

    pub fn workspace_replica_snapshots(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<WorkspaceReplicaSnapshotInfo>> {
        let c = self.connection()?;
        let mut q=db(c.prepare("SELECT snapshot_id FROM workspace_replica_snapshots WHERE workspace_id=?1 ORDER BY imported_at,snapshot_id LIMIT 512"))?;
        let ids = db(db(q.query_map([workspace_id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| snapshot_info_row(&c, id)).collect()
    }

    pub fn workspace_replica_divergences(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<WorkspaceReplicaDivergenceInfo>> {
        let c = self.connection()?;
        divergence_rows(&c, workspace_id)
    }

    pub fn replica_writer_fence(
        &self,
        source_node_id: &str,
        workspace_id: &str,
        event: &WorkspaceWriterEventInfo,
    ) -> Result<WorkspaceWriterInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let control: Option<String> = db(tx
            .query_row(
                "SELECT control_node_id FROM workspace_replica_origins WHERE workspace_id=?1",
                [workspace_id],
                |r| r.get(0),
            )
            .optional())?;
        if control.as_deref() != Some(source_node_id) {
            return Err(Error::Denied(
                "only the recorded Workspace control AwNode may fence a replica writer",
            ));
        }
        let current = workspace_writer::writer_row(&tx, workspace_id)?;
        workspace_writer::require_local_writer(&tx, &current)?;
        if event.workspace_id != workspace_id
            || event.domain_id != current.domain_id
            || event.old_writer_node_id.as_deref() != Some(current.writer_node_id.as_str())
            || event.old_writer_epoch != Some(current.writer_epoch)
            || event.new_writer_node_id != source_node_id
            || event.new_writer_epoch != current.writer_epoch.saturating_add(1)
            || event.authority_term < current.authority_term
        {
            return Err(Error::Conflict(
                "Workspace writer fence event does not exactly advance the current replica writer",
            ));
        }
        let changed = db(tx.execute(
            "UPDATE workspace_writer_state SET writer_node_id=?1,writer_epoch=?2,authority_term=?3,state='active',revision=revision+1,updated_at=?4 WHERE workspace_id=?5 AND writer_node_id=?6 AND writer_epoch=?7",
            params![source_node_id,event.new_writer_epoch,event.authority_term,now(),workspace_id,current.writer_node_id,current.writer_epoch],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "Workspace replica writer changed while applying the fence",
            ));
        }
        insert_or_verify_writer_event(&tx, event)?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "workspace.replica_writer_fenced",
            "workspace",
            workspace_id,
            None,
            serde_json::json!({
                "control_node_id":source_node_id,
                "old_writer_node_id":current.writer_node_id,
                "old_writer_epoch":current.writer_epoch,
                "new_writer_node_id":event.new_writer_node_id,
                "new_writer_epoch":event.new_writer_epoch,
                "authority_term":event.authority_term,
                "writer_event_id":event.event_id
            }),
        )?;
        let writer = workspace_writer::writer_row(&tx, workspace_id)?;
        db(tx.commit())?;
        Ok(writer)
    }

    pub fn replica_writer_branches(
        &self,
        source_node_id: &str,
        actor: &PrincipalId,
        workspace_id: &str,
        writer_epoch: u64,
        authority_term: u64,
        session_id: &str,
    ) -> Result<Vec<BranchInfo>> {
        let c = self.connection()?;
        require_replica_control(
            &c,
            source_node_id,
            workspace_id,
            writer_epoch,
            authority_term,
        )?;
        authorize(&c, session_id, actor, false)?;
        let actual: String = db(c.query_row(
            "SELECT workspace_id FROM sessions WHERE id=?1",
            [session_id],
            |r| r.get(0),
        ))?;
        if actual != workspace_id {
            return Err(Error::Denied(
                "Session is outside the controlled Workspace replica",
            ));
        }
        let mut q=db(c.prepare("SELECT id,session_id,name,head,revision,active_run FROM branches WHERE session_id=?1 ORDER BY created_at,id"))?;
        let rows = db(q.query_map([session_id], |r| {
            Ok(BranchInfo {
                id: r.get(0)?,
                session_id: r.get(1)?,
                name: r.get(2)?,
                head: r.get(3)?,
                revision: r.get(4)?,
                active_run: r.get(5)?,
            })
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn replica_writer_checkpoint(
        &self,
        source_node_id: &str,
        actor: &PrincipalId,
        workspace_id: &str,
        writer_epoch: u64,
        authority_term: u64,
        session_id: &str,
        branch_id: &str,
        expected_head: &str,
        expected_revision: u64,
        summary: &str,
    ) -> Result<BranchInfo> {
        if summary.trim().is_empty() || summary.len() > aw_agent_core::limits::MAX_PROMPT_BYTES {
            return Err(Error::Limit("checkpoint needs 1..131072 UTF-8 bytes"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        require_replica_control(
            &tx,
            source_node_id,
            workspace_id,
            writer_epoch,
            authority_term,
        )?;
        authorize(&tx, session_id, actor, true)?;
        let actual: String = db(tx.query_row(
            "SELECT workspace_id FROM sessions WHERE id=?1",
            [session_id],
            |r| r.get(0),
        ))?;
        if actual != workspace_id {
            return Err(Error::Denied(
                "Session is outside the controlled Workspace replica",
            ));
        }
        let b = branch_in(&tx, session_id, branch_id)?;
        if b.head != expected_head || b.revision != expected_revision || b.active_run.is_some() {
            return Err(Error::Conflict(
                "remote writer branch head/revision changed; reload without rebasing",
            ));
        }
        context::authorize_node(&tx, session_id, &b.head, actor)?;
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM nodes WHERE session_id=?1",
            [session_id],
            |r| r.get(0),
        ))?;
        if count >= MAX_SESSION_NODES as i64 {
            return Err(Error::Limit("session node limit reached"));
        }
        let node = id("n")?;
        insert_node(
            &tx,
            session_id,
            &node,
            Some(&b.head),
            "checkpoint",
            actor,
            serde_json::json!({"summary":summary,"authored_by_user":true,"workspace_writer_epoch":writer_epoch}),
        )?;
        let changed=db(tx.execute("UPDATE branches SET head=?1,revision=revision+1 WHERE id=?2 AND head=?3 AND revision=?4 AND active_run IS NULL",params![node,b.id,b.head,b.revision]))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "remote writer branch moved during checkpoint",
            ));
        }
        let txid = durability::tx_id()?;
        durability::ref_event(
            &tx,
            &b,
            &node,
            b.revision + 1,
            actor,
            "remote_writer_checkpoint",
            Some(source_node_id),
            &txid,
        )?;
        db(tx.execute(
            "UPDATE sessions SET updated_at=?1 WHERE id=?2",
            params![now(), session_id],
        ))?;
        audit(
            &tx,
            session_id,
            actor,
            "workspace.remote_writer_checkpoint",
            serde_json::json!({"source_node_id":source_node_id,"workspace_id":workspace_id,"writer_epoch":writer_epoch,"authority_term":authority_term,"previous_head":b.head,"node":node}),
        )?;
        let out = branch_in(&tx, session_id, &b.id)?;
        db(tx.commit())?;
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn replica_writer_fork(
        &self,
        source_node_id: &str,
        actor: &PrincipalId,
        workspace_id: &str,
        writer_epoch: u64,
        authority_term: u64,
        session_id: &str,
        name: &str,
        at: &str,
    ) -> Result<BranchInfo> {
        label(name, 80)?;
        if name.starts_with("b_") {
            return Err(Error::Config(
                "branch names beginning b_ are reserved for IDs".into(),
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        require_replica_control(
            &tx,
            source_node_id,
            workspace_id,
            writer_epoch,
            authority_term,
        )?;
        authorize(&tx, session_id, actor, true)?;
        let actual: String = db(tx.query_row(
            "SELECT workspace_id FROM sessions WHERE id=?1",
            [session_id],
            |r| r.get(0),
        ))?;
        if actual != workspace_id {
            return Err(Error::Denied(
                "Session is outside the controlled Workspace replica",
            ));
        }
        context::authorize_node(&tx, session_id, at, actor)?;
        let kind: Option<String> = db(tx
            .query_row(
                "SELECT kind FROM nodes WHERE session_id=?1 AND id=?2",
                params![session_id, at],
                |r| r.get(0),
            )
            .optional())?;
        if matches!(kind.as_deref(), None | Some("prompt")) {
            return Err(Error::Conflict(
                "fork requires a root, finished attempt or checkpoint node",
            ));
        }
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM branches WHERE session_id=?1",
            [session_id],
            |r| r.get(0),
        ))?;
        if count >= 1024 {
            return Err(Error::Limit("session branch limit reached"));
        }
        let branch = id("b")?;
        db(tx.execute(
            "INSERT INTO branches(id,session_id,name,head,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![branch, session_id, name, at, now()],
        ))?;
        let created = branch_in(&tx, session_id, &branch)?;
        let txid = durability::tx_id()?;
        durability::ref_event(
            &tx,
            &BranchInfo {
                id: created.id.clone(),
                session_id: created.session_id.clone(),
                name: created.name.clone(),
                head: at.into(),
                revision: 0,
                active_run: None,
            },
            at,
            0,
            actor,
            "remote_writer_branch_created",
            Some(source_node_id),
            &txid,
        )?;
        audit(
            &tx,
            session_id,
            actor,
            "workspace.remote_writer_fork",
            serde_json::json!({"source_node_id":source_node_id,"workspace_id":workspace_id,"writer_epoch":writer_epoch,"authority_term":authority_term,"branch":branch,"at":at}),
        )?;
        let out = branch_in(&tx, session_id, &branch)?;
        db(tx.commit())?;
        Ok(out)
    }
}

fn require_replica_control(
    c: &Connection,
    source_node_id: &str,
    workspace_id: &str,
    writer_epoch: u64,
    authority_term: u64,
) -> Result<WorkspaceWriterInfo> {
    let control: Option<String> = db(c
        .query_row(
            "SELECT control_node_id FROM workspace_replica_origins WHERE workspace_id=?1",
            [workspace_id],
            |r| r.get(0),
        )
        .optional())?;
    if control.as_deref() != Some(source_node_id) {
        return Err(Error::Denied(
            "authenticated peer is not the Workspace replica control AwNode",
        ));
    }
    let writer = workspace_writer::writer_row(c, workspace_id)?;
    workspace_writer::require_local_writer(c, &writer)?;
    if writer.writer_epoch != writer_epoch || writer.authority_term != authority_term {
        return Err(Error::Conflict(
            "Workspace writer epoch/authority term changed; reload before remote ref mutation",
        ));
    }
    Ok(writer)
}
