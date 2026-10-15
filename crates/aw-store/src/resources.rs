use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Hosted,
    FleetVirtual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainInfo {
    pub id: String,
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
pub struct WorkspaceInfo {
    pub id: String,
    pub domain_id: String,
    pub name: String,
    pub kind: WorkspaceKind,
    pub origin_kind: String,
    pub host_machine_id: Option<String>,
    pub policy_revision: u64,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineInfo {
    pub id: String,
    pub owning_domain_id: Option<String>,
    pub node_id: String,
    pub display_name: String,
    pub kind: String,
    pub status: String,
    pub key_fingerprint: Option<String>,
    pub labels: serde_json::Value,
    pub capabilities: serde_json::Value,
    pub capability_revision: u64,
    pub security_epoch: u64,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwNetPeerInfo {
    pub id: String,
    pub node_id: String,
    pub public_key: String,
    pub display_name: String,
    pub trust_state: String,
    pub security_epoch: u64,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainFederationInfo {
    pub id: String,
    pub local_domain_id: String,
    pub remote_peer_id: String,
    pub remote_domain_id: String,
    pub direction: String,
    pub scope: serde_json::Value,
    pub permissions: serde_json::Value,
    pub include_child_domains: bool,
    pub allow_redelegation: bool,
    pub policy_epoch: u64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederationLeaseInfo {
    pub id: String,
    pub federation_id: String,
    pub remote_peer_id: String,
    pub issued_to_node_id: String,
    pub scope: serde_json::Value,
    pub permissions: serde_json::Value,
    pub policy_epoch: u64,
    pub domain_policy_revision: u64,
    pub domain_security_epoch: u64,
    pub peer_security_epoch: u64,
    pub expires_at: i64,
    pub status: String,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteSessionRef {
    pub id: String,
    pub operation_id: String,
    pub request_sha256: String,
    pub source_workspace_id: String,
    pub source_session_id: Option<String>,
    pub peer_node_id: String,
    pub remote_store_id: Option<String>,
    pub remote_workspace_id: Option<String>,
    pub remote_session_id: Option<String>,
    pub remote_branch_id: Option<String>,
    pub state: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerSessionPlacement {
    pub workspace: WorkspaceInfo,
    pub session: SessionInfo,
    pub branch: BranchInfo,
    pub reused: bool,
}

struct RemoteSessionOriginRow {
    session_id: String,
    source_domain_id: String,
    source_workspace_id: String,
    source_session_id: Option<String>,
    source_branch_id: Option<String>,
    request_sha256: String,
    source_node_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteSessionExecutionInfo {
    pub source_node_id: String,
    pub execution_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub prompt_sha256: String,
    pub owner_request: serde_json::Value,
    pub state: String,
    pub run_id: Option<u64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetInfo {
    pub id: String,
    pub domain_id: String,
    pub name: String,
    pub description: Option<String>,
    pub membership_revision: u64,
    pub default_execution_policy: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetMemberInfo {
    pub fleet_id: String,
    pub machine_id: String,
    pub state: String,
    pub joined_at: i64,
    pub removed_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetOperationInfo {
    pub operation_id: String,
    pub fleet_id: String,
    pub source_workspace_id: Option<String>,
    pub request_sha256: String,
    pub membership_revision: u64,
    pub execution_policy: String,
    pub state: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetOperationMemberInfo {
    pub operation_id: String,
    pub machine_id: String,
    pub ordinal: u64,
    pub state: String,
    pub remote_ref_id: Option<String>,
    pub result_sha256: Option<String>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetOperationSnapshot {
    pub operation: FleetOperationInfo,
    pub members: Vec<FleetOperationMemberInfo>,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionExecutionTarget {
    Local {
        machine_id: String,
    },
    Machine {
        machine_id: String,
    },
    Fleet {
        fleet_id: String,
        #[serde(default)]
        membership_revision: Option<u64>,
    },
}

pub(crate) fn session_target_in(c: &Connection, session: &str) -> Result<SessionExecutionTarget> {
    let row = db(c
        .query_row(
            "SELECT target_kind,machine_id,fleet_id,target_snapshot_json
             FROM session_execution_targets WHERE session_id=?1",
            [session],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional())?
    .ok_or(Error::Storage("session execution target missing"))?;
    let snapshot: serde_json::Value = decode(&row.3)?;
    match row.0.as_str() {
        "local" => Ok(SessionExecutionTarget::Local {
            machine_id: row
                .1
                .ok_or(Error::Storage("local target missing machine"))?,
        }),
        "machine" => Ok(SessionExecutionTarget::Machine {
            machine_id: row
                .1
                .ok_or(Error::Storage("machine target missing machine"))?,
        }),
        "fleet" => Ok(SessionExecutionTarget::Fleet {
            fleet_id: row.2.ok_or(Error::Storage("fleet target missing fleet"))?,
            membership_revision: snapshot
                .get("membership_revision")
                .and_then(serde_json::Value::as_u64),
        }),
        _ => Err(Error::Storage("unknown session target")),
    }
}

fn workspace_kind(raw: &str) -> Result<WorkspaceKind> {
    match raw {
        "hosted" => Ok(WorkspaceKind::Hosted),
        "fleet_virtual" => Ok(WorkspaceKind::FleetVirtual),
        _ => Err(Error::Storage("unknown stored workspace kind")),
    }
}

fn current_user(c: &Connection, actor: &PrincipalId) -> Result<String> {
    ensure_user_for_principal(c, actor)
}
fn current_user_with_default(c: &Connection, actor: &PrincipalId) -> Result<String> {
    let user = current_user(c, actor)?;
    let realm: String = db(c.query_row(
        "SELECT realm_id FROM users WHERE user_id=?1",
        [&user],
        |r| r.get(0),
    ))?;
    let _ = ensure_default_domain_workspace(c, &user, &realm)?;
    Ok(user)
}

fn domain_row(c: &Connection, id: &str) -> Result<DomainInfo> {
    db(c.query_row("SELECT domain_id,parent_domain_id,name,status,policy_revision,security_epoch,created_at,updated_at FROM domains WHERE domain_id=?1",[id],|r|Ok(DomainInfo{id:r.get(0)?,parent_domain_id:r.get(1)?,name:r.get(2)?,status:r.get(3)?,policy_revision:r.get(4)?,security_epoch:r.get(5)?,created_at:r.get(6)?,updated_at:r.get(7)?})).optional())?.ok_or(Error::Denied("domain unavailable or not permitted"))
}

fn workspace_row(c: &Connection, id: &str) -> Result<WorkspaceInfo> {
    let row=db(c.query_row("SELECT workspace_id,domain_id,name,kind,origin_kind,host_machine_id,policy_revision,revision,created_at,updated_at,archived_at FROM workspaces WHERE workspace_id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,u64>(6)?,r.get::<_,u64>(7)?,r.get::<_,i64>(8)?,r.get::<_,i64>(9)?,r.get::<_,Option<i64>>(10)?))).optional())?.ok_or(Error::Denied("workspace unavailable or not permitted"))?;
    Ok(WorkspaceInfo {
        id: row.0,
        domain_id: row.1,
        name: row.2,
        kind: workspace_kind(&row.3)?,
        origin_kind: row.4,
        host_machine_id: row.5,
        policy_revision: row.6,
        revision: row.7,
        created_at: row.8,
        updated_at: row.9,
        archived_at: row.10,
    })
}

fn machine_row(c: &Connection, id: &str) -> Result<MachineInfo> {
    let row=db(c.query_row("SELECT machine_id,owning_domain_id,node_id,display_name,kind,status,key_fingerprint,labels_json,capabilities_json,capability_revision,security_epoch,created_at,last_seen_at FROM machines WHERE machine_id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,u64>(9)?,r.get::<_,u64>(10)?,r.get::<_,i64>(11)?,r.get::<_,Option<i64>>(12)?))).optional())?.ok_or(Error::Denied("machine unavailable or not permitted"))?;
    Ok(MachineInfo {
        id: row.0,
        owning_domain_id: row.1,
        node_id: row.2,
        display_name: row.3,
        kind: row.4,
        status: row.5,
        key_fingerprint: row.6,
        labels: decode(&row.7)?,
        capabilities: decode(&row.8)?,
        capability_revision: row.9,
        security_epoch: row.10,
        created_at: row.11,
        last_seen_at: row.12,
    })
}

fn fleet_row(c: &Connection, id: &str) -> Result<FleetInfo> {
    db(c.query_row("SELECT fleet_id,domain_id,name,description,membership_revision,default_execution_policy,created_at,updated_at FROM fleets WHERE fleet_id=?1",[id],|r|Ok(FleetInfo{id:r.get(0)?,domain_id:r.get(1)?,name:r.get(2)?,description:r.get(3)?,membership_revision:r.get(4)?,default_execution_policy:r.get(5)?,created_at:r.get(6)?,updated_at:r.get(7)?})).optional())?.ok_or(Error::Denied("fleet unavailable or not permitted"))
}

fn fleet_operation_row(c: &Connection, id: &str) -> Result<FleetOperationInfo> {
    db(c.query_row(
        "SELECT operation_id,fleet_id,source_workspace_id,request_sha256,membership_revision,execution_policy,state,created_at,updated_at FROM fleet_operations WHERE operation_id=?1",
        [id],
        |r| Ok(FleetOperationInfo {
            operation_id: r.get(0)?,
            fleet_id: r.get(1)?,
            source_workspace_id: r.get(2)?,
            request_sha256: r.get(3)?,
            membership_revision: r.get(4)?,
            execution_policy: r.get(5)?,
            state: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        }),
    ).optional())?.ok_or(Error::Denied("fleet operation unavailable or not permitted"))
}

fn fleet_operation_members_rows(c: &Connection, id: &str) -> Result<Vec<FleetOperationMemberInfo>> {
    let mut q = db(c.prepare(
        "SELECT operation_id,machine_id,ordinal,state,remote_ref_id,result_sha256,updated_at FROM fleet_operation_members WHERE operation_id=?1 ORDER BY ordinal",
    ))?;
    let rows = db(q.query_map([id], |r| {
        Ok(FleetOperationMemberInfo {
            operation_id: r.get(0)?,
            machine_id: r.get(1)?,
            ordinal: r.get(2)?,
            state: r.get(3)?,
            remote_ref_id: r.get(4)?,
            result_sha256: r.get(5)?,
            updated_at: r.get(6)?,
        })
    }))?;
    db(rows.collect::<rusqlite::Result<Vec<_>>>())
}

fn federation_row(c: &Connection, id: &str) -> Result<DomainFederationInfo> {
    let row = db(c
        .query_row(
            "SELECT federation_id,local_domain_id,remote_peer_id,remote_domain_id,direction,scope_json,permissions_json,include_child_domains,allow_redelegation,policy_epoch,status,created_at,updated_at FROM domain_federations WHERE federation_id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, u64>(9)?,
                    r.get::<_, String>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, i64>(12)?,
                ))
            },
        )
        .optional())?
        .ok_or(Error::Denied("federation unavailable or not permitted"))?;
    Ok(DomainFederationInfo {
        id: row.0,
        local_domain_id: row.1,
        remote_peer_id: row.2,
        remote_domain_id: row.3,
        direction: row.4,
        scope: decode(&row.5)?,
        permissions: decode(&row.6)?,
        include_child_domains: row.7 != 0,
        allow_redelegation: row.8 != 0,
        policy_epoch: row.9,
        status: row.10,
        created_at: row.11,
        updated_at: row.12,
    })
}

fn federation_lease_row(c: &Connection, id: &str) -> Result<FederationLeaseInfo> {
    let row = db(c
        .query_row(
            "SELECT lease_id,federation_id,remote_peer_id,issued_to_node_id,scope_json,permissions_json,policy_epoch,domain_policy_revision,domain_security_epoch,peer_security_epoch,expires_at,status,created_at,revoked_at FROM federation_capability_leases WHERE lease_id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, u64>(6)?,
                    r.get::<_, u64>(7)?,
                    r.get::<_, u64>(8)?,
                    r.get::<_, u64>(9)?,
                    r.get::<_, i64>(10)?,
                    r.get::<_, String>(11)?,
                    r.get::<_, i64>(12)?,
                    r.get::<_, Option<i64>>(13)?,
                ))
            },
        )
        .optional())?
        .ok_or(Error::Denied("federation lease unavailable or not permitted"))?;
    Ok(FederationLeaseInfo {
        id: row.0,
        federation_id: row.1,
        remote_peer_id: row.2,
        issued_to_node_id: row.3,
        scope: decode(&row.4)?,
        permissions: decode(&row.5)?,
        policy_epoch: row.6,
        domain_policy_revision: row.7,
        domain_security_epoch: row.8,
        peer_security_epoch: row.9,
        expires_at: row.10,
        status: row.11,
        created_at: row.12,
        revoked_at: row.13,
    })
}

fn validate_federation_documents(
    scope: &serde_json::Value,
    permissions: &serde_json::Value,
) -> Result<()> {
    if !scope.is_object() {
        return Err(Error::Config(
            "federation scope must be a JSON object".into(),
        ));
    }
    let Some(actions) = permissions.as_array() else {
        return Err(Error::Config(
            "federation permissions must be a JSON array of action strings".into(),
        ));
    };
    if actions.is_empty() || actions.len() > 128 {
        return Err(Error::Config(
            "federation permissions must contain 1..128 actions".into(),
        ));
    }
    for action in actions {
        let Some(action) = action.as_str() else {
            return Err(Error::Config("federation action must be a string".into()));
        };
        label(action, 128)?;
    }
    if json(scope)?.len() > 64 * 1024 || json(permissions)?.len() > 64 * 1024 {
        return Err(Error::Limit("federation scope/permissions exceed 64 KiB"));
    }
    Ok(())
}

pub(crate) fn ensure_default_domain_workspace(
    c: &Connection,
    user: &str,
    legacy_realm: &str,
) -> Result<(String, String)> {
    let store: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |r| r.get(0),
    ))?;
    let domain:Option<String>=db(c.query_row("SELECT d.domain_id FROM domains d JOIN domain_memberships m ON m.domain_id=d.domain_id WHERE m.user_id=?1 AND m.status='active' ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END,d.created_at,d.domain_id LIMIT 1",[user],|r|r.get(0)).optional())?;
    let domain = if let Some(id) = domain {
        id
    } else {
        let found:Option<String>=db(c.query_row("SELECT domain_id FROM domains WHERE authority_store_id=?1 AND legacy_realm=?2 AND status='active' ORDER BY created_at,domain_id LIMIT 1",params![store,legacy_realm],|r|r.get(0)).optional())?;
        let id = if let Some(id) = found {
            id
        } else {
            let id = id("dom")?;
            let name = if legacy_realm == "local" {
                "Personal"
            } else {
                legacy_realm
            };
            db(c.execute("INSERT INTO domains(domain_id,authority_store_id,name,legacy_realm,status,created_at,updated_at) VALUES(?1,?2,?3,?4,'active',?5,?5)",params![id,store,name,legacy_realm,now()]))?;
            id
        };
        db(c.execute("INSERT INTO domain_memberships(domain_id,user_id,role,status,joined_at) VALUES(?1,?2,'owner','active',?3) ON CONFLICT(domain_id,user_id) DO UPDATE SET status='active'",params![id,user,now()]))?;
        id
    };
    authority::initialize_domain_authority_if_available(c, &domain)?;
    authority::initialize_domain_authority_if_available(c, &domain)?;
    let workspace:Option<String>=db(c.query_row("SELECT workspace_id FROM workspaces WHERE domain_id=?1 AND name='local' AND archived_at IS NULL ORDER BY created_at,workspace_id LIMIT 1",[&domain],|r|r.get(0)).optional())?;
    let workspace = if let Some(id) = workspace {
        id
    } else {
        let machine: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='local_machine_id'",
            [],
            |r| r.get(0),
        ))?;
        let id = id("ws")?;
        db(c.execute("INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,host_machine_id,created_by_user_id,created_at,updated_at) VALUES(?1,?2,'local','hosted','builtin_local',?3,?4,?5,?5)",params![id,domain,machine,user,now()]))?;
        id
    };
    Ok((domain, workspace))
}

fn stable_peer_workspace_name(
    source_store_id: &str,
    source_domain_id: &str,
    source_workspace_id: &str,
    source_workspace_name: &str,
) -> Result<String> {
    for value in [source_store_id, source_domain_id, source_workspace_id] {
        label(value, 160)?;
    }
    label(source_workspace_name, 128)?;
    let mut digest = Sha256::new();
    for value in [source_store_id, source_domain_id, source_workspace_id] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    let digest = format!("{:x}", digest.finalize());
    let mut base = source_workspace_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect::<String>();
    while base.contains("--") {
        base = base.replace("--", "-");
    }
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "workspace" } else { base };
    let name = format!("peer:{base}-{}", &digest[..16]);
    label(&name, 128)?;
    Ok(name)
}

fn ensure_peer_workspace_tx(
    c: &Connection,
    actor: &PrincipalId,
    source_store_id: &str,
    source_domain_id: &str,
    source_workspace_id: &str,
    source_workspace_name: &str,
    source_node_id: &str,
) -> Result<(WorkspaceInfo, bool)> {
    let name = stable_peer_workspace_name(
        source_store_id,
        source_domain_id,
        source_workspace_id,
        source_workspace_name,
    )?;
    label(source_node_id, 160)?;
    let existing: Option<String> = db(c
        .query_row(
            "SELECT p.workspace_id FROM peer_workspace_origins p JOIN workspaces w ON w.workspace_id=p.workspace_id WHERE p.source_store_id=?1 AND p.source_domain_id=?2 AND p.source_workspace_id=?3 AND w.archived_at IS NULL",
            params![source_store_id, source_domain_id, source_workspace_id],
            |r| r.get(0),
        )
        .optional())?;
    if let Some(workspace) = existing {
        return Ok((workspace_row(c, &workspace)?, false));
    }
    let user = current_user(c, actor)?;
    let realm: String = db(c.query_row(
        "SELECT realm_id FROM users WHERE user_id=?1",
        [&user],
        |r| r.get(0),
    ))?;
    let (domain, _) = ensure_default_domain_workspace(c, &user, &realm)?;
    let collision: Option<String> = db(c
        .query_row(
            "SELECT workspace_id FROM workspaces WHERE domain_id=?1 AND name=?2 AND archived_at IS NULL",
            params![domain, name],
            |r| r.get(0),
        )
        .optional())?;
    if collision.is_some() {
        return Err(Error::Conflict(
            "stable PeerWorkspace name collision; preserve source identity and inspect",
        ));
    }
    let machine: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='local_machine_id'",
        [],
        |r| r.get(0),
    ))?;
    let workspace = id("ws")?;
    let created = now();
    db(c.execute(
        "INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,host_machine_id,created_by_user_id,created_at,updated_at) VALUES(?1,?2,?3,'hosted','peer_created',?4,?5,?6,?6)",
        params![workspace, domain, name, machine, user, created],
    ))?;
    db(c.execute(
        "INSERT INTO peer_workspace_origins(workspace_id,source_store_id,source_domain_id,source_workspace_id,source_machine_id,source_display_name,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            workspace,
            source_store_id,
            source_domain_id,
            source_workspace_id,
            source_node_id,
            source_workspace_name,
            created
        ],
    ))?;
    Ok((workspace_row(c, &workspace)?, true))
}

fn remote_session_ref_row(c: &Connection, selector: &str) -> Result<RemoteSessionRef> {
    db(c.query_row(
        "SELECT r.ref_id,r.operation_id,r.request_sha256,r.source_workspace_id,r.source_session_id,p.node_id,r.remote_store_id,r.remote_workspace_id,r.remote_session_id,r.remote_branch_id,r.state,r.created_at,r.updated_at FROM remote_session_refs r JOIN awnet_peers p ON p.peer_id=r.remote_peer_id WHERE r.ref_id=?1 OR r.operation_id=?1 ORDER BY CASE WHEN r.ref_id=?1 THEN 0 ELSE 1 END LIMIT 1",
        [selector],
        |r| {
            Ok(RemoteSessionRef {
                id: r.get(0)?,
                operation_id: r.get(1)?,
                request_sha256: r.get(2)?,
                source_workspace_id: r.get(3)?,
                source_session_id: r.get(4)?,
                peer_node_id: r.get(5)?,
                remote_store_id: r.get(6)?,
                remote_workspace_id: r.get(7)?,
                remote_session_id: r.get(8)?,
                remote_branch_id: r.get(9)?,
                state: r.get(10)?,
                created_at: r.get(11)?,
                updated_at: r.get(12)?,
            })
        },
    )
    .optional())?
    .ok_or(Error::Denied("remote Session reference unavailable or not permitted"))
}

fn authorized_remote_session_ref(
    c: &Connection,
    actor: &PrincipalId,
    selector: &str,
) -> Result<RemoteSessionRef> {
    let user = current_user_with_default(c, actor)?;
    let id: Option<String> = db(c
        .query_row(
            "SELECT r.ref_id FROM remote_session_refs r JOIN workspaces w ON w.workspace_id=r.source_workspace_id JOIN domain_memberships dm ON dm.domain_id=w.domain_id WHERE dm.user_id=?1 AND dm.status='active' AND (r.ref_id=?2 OR r.operation_id=?2) ORDER BY CASE WHEN r.ref_id=?2 THEN 0 ELSE 1 END LIMIT 1",
            params![user, selector],
            |r| r.get(0),
        )
        .optional())?;
    remote_session_ref_row(
        c,
        id.as_deref().ok_or(Error::Denied(
            "remote Session reference unavailable or not permitted",
        ))?,
    )
}

fn remote_session_execution_row(
    c: &Connection,
    source_node_id: &str,
    execution_id: &str,
) -> Result<RemoteSessionExecutionInfo> {
    let row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<i64>,
        i64,
        i64,
    ) = db(c.query_row(
        "SELECT source_node_id,execution_id,session_id,branch_id,prompt_sha256,owner_request_json,state,run_id,created_at,updated_at FROM remote_session_executions WHERE source_node_id=?1 AND execution_id=?2",
        params![source_node_id, execution_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        },
    ))?;
    Ok(RemoteSessionExecutionInfo {
        source_node_id: row.0,
        execution_id: row.1,
        session_id: row.2,
        branch_id: row.3,
        prompt_sha256: row.4,
        owner_request: decode(&row.5)?,
        state: row.6,
        run_id: row.7.map(|v| v as u64),
        created_at: row.8,
        updated_at: row.9,
    })
}

impl SessionStore {
    pub fn domains(&self, actor: &PrincipalId) -> Result<Vec<DomainInfo>> {
        let c = self.connection()?;
        let user = current_user_with_default(&c, actor)?;
        let mut q=db(c.prepare("SELECT d.domain_id FROM domains d JOIN domain_memberships m ON m.domain_id=d.domain_id WHERE m.user_id=?1 AND m.status='active' AND d.status='active' ORDER BY d.name,d.domain_id"))?;
        let ids = db(db(q.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| domain_row(&c, id)).collect()
    }
    pub fn domain(&self, actor: &PrincipalId, selector: &str) -> Result<DomainInfo> {
        let c = self.connection()?;
        let user = current_user_with_default(&c, actor)?;
        let mut q=db(c.prepare("SELECT d.domain_id FROM domains d JOIN domain_memberships m ON m.domain_id=d.domain_id WHERE m.user_id=?1 AND m.status='active' AND d.status='active' AND (d.domain_id=?2 OR d.name=?2) ORDER BY CASE WHEN d.domain_id=?2 THEN 0 ELSE 1 END,d.domain_id LIMIT 2"))?;
        let ids = db(
            db(q.query_map(params![user, selector], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        if ids.is_empty() {
            return Err(Error::Denied("domain unavailable or not permitted"));
        }
        if ids.len() > 1 && ids[0] != selector {
            return Err(Error::Conflict(
                "Domain name is ambiguous; use its opaque ID",
            ));
        }
        domain_row(&c, &ids[0])
    }
    pub fn default_domain(&self, actor: &PrincipalId) -> Result<DomainInfo> {
        let c = self.connection()?;
        let user = current_user(&c, actor)?;
        let realm: String = db(c.query_row(
            "SELECT realm_id FROM users WHERE user_id=?1",
            [&user],
            |r| r.get(0),
        ))?;
        let preferred:Option<String>=db(c.query_row("SELECT d.domain_id FROM domains d JOIN domain_memberships m ON m.domain_id=d.domain_id WHERE m.user_id=?1 AND m.status='active' AND d.status='active' AND d.legacy_realm=?2 ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END,d.created_at,d.domain_id LIMIT 1",params![user,realm],|r|r.get(0)).optional())?;
        if let Some(id) = preferred {
            return domain_row(&c, &id);
        }
        drop(c);
        self.domains(actor)?
            .into_iter()
            .next()
            .ok_or(Error::Denied("no accessible domain"))
    }
    pub fn create_domain(
        &self,
        actor: &PrincipalId,
        name: &str,
        parent_selector: Option<&str>,
    ) -> Result<DomainInfo> {
        label(name, 96)?;
        let parent = if let Some(p) = parent_selector {
            Some(self.domain(actor, p)?)
        } else {
            None
        };
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let store: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))?;
        if let Some(p) = &parent {
            let role:Option<String>=db(tx.query_row("SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",params![p.id,user],|r|r.get(0)).optional())?;
            if !matches!(role.as_deref(), Some("owner" | "admin")) {
                return Err(Error::Denied(
                    "child Domain creation requires parent owner/admin authority",
                ));
            }
            authority::require_domain_authority_write(&tx, &p.id)?;
        }
        let parent_id = parent.as_ref().map(|p| p.id.as_str());
        let exists:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM domains WHERE authority_store_id=?1 AND ((parent_domain_id IS NULL AND ?2 IS NULL) OR parent_domain_id=?2) AND name=?3 AND status<>'archived')",params![store,parent_id,name],|r|r.get(0)))?;
        if exists {
            return Err(Error::Conflict(
                "Domain name already exists under this parent",
            ));
        }
        let domain = id("dom")?;
        let created = now();
        db(tx.execute("INSERT INTO domains(domain_id,authority_store_id,parent_domain_id,name,status,created_at,updated_at) VALUES(?1,?2,?3,?4,'active',?5,?5)",params![domain,store,parent_id,name,created]))?;
        db(tx.execute("INSERT INTO domain_memberships(domain_id,user_id,role,status,joined_at) VALUES(?1,?2,'owner','active',?3)",params![domain,user,created]))?;
        authority::initialize_domain_authority(&tx, &domain)?;
        let machine: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_machine_id'",
            [],
            |r| r.get(0),
        ))?;
        let workspace = id("ws")?;
        db(tx.execute("INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,host_machine_id,created_by_user_id,created_at,updated_at) VALUES(?1,?2,'local','hosted','builtin_local',?3,?4,?5,?5)",params![workspace,domain,machine,user,created]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "domain.created",
            "domain",
            &domain,
            Some(actor),
            serde_json::json!({"domain_id":domain,"workspace_id":workspace}),
        )?;
        db(tx.commit())?;
        domain_row(&c, &domain)
    }
    pub fn workspaces(
        &self,
        actor: &PrincipalId,
        domain: Option<&str>,
    ) -> Result<Vec<WorkspaceInfo>> {
        let c = self.connection()?;
        let user = current_user_with_default(&c, actor)?;
        let mut q=db(c.prepare("SELECT w.workspace_id FROM workspaces w JOIN domain_memberships m ON m.domain_id=w.domain_id WHERE m.user_id=?1 AND m.status='active' AND w.archived_at IS NULL AND (?2 IS NULL OR w.domain_id=?2) ORDER BY w.name,w.workspace_id"))?;
        let ids = db(
            db(q.query_map(params![user, domain], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        ids.iter().map(|id| workspace_row(&c, id)).collect()
    }
    pub fn workspace(
        &self,
        actor: &PrincipalId,
        domain: Option<&str>,
        selector: &str,
    ) -> Result<WorkspaceInfo> {
        let c = self.connection()?;
        let user = current_user_with_default(&c, actor)?;
        let mut q=db(c.prepare("SELECT w.workspace_id FROM workspaces w JOIN domain_memberships m ON m.domain_id=w.domain_id WHERE m.user_id=?1 AND m.status='active' AND w.archived_at IS NULL AND (?2 IS NULL OR w.domain_id=?2) AND (w.workspace_id=?3 OR w.name=?3) ORDER BY CASE WHEN w.workspace_id=?3 THEN 0 ELSE 1 END,w.workspace_id LIMIT 2"))?;
        let ids = db(
            db(q.query_map(params![user, domain, selector], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        if ids.is_empty() {
            return Err(Error::Denied("workspace unavailable or not permitted"));
        }
        if ids.len() > 1 && ids[0] != selector {
            return Err(Error::Conflict(
                "Workspace name is ambiguous; specify --domain or opaque ID",
            ));
        }
        workspace_row(&c, &ids[0])
    }
    pub fn local_workspace(
        &self,
        actor: &PrincipalId,
        domain: Option<&str>,
    ) -> Result<WorkspaceInfo> {
        let d = if let Some(selector) = domain {
            self.domain(actor, selector)?
        } else {
            self.default_domain(actor)?
        };
        self.workspace(actor, Some(&d.id), "local")
    }
    pub fn create_workspace(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        name: &str,
    ) -> Result<WorkspaceInfo> {
        label(name, 128)?;
        if name == "local" {
            return Err(Error::Conflict("local is the reserved default workspace"));
        }
        let domain = if let Some(s) = domain_selector {
            self.domain(actor, s)?
        } else {
            self.default_domain(actor)?
        };
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let role:Option<String>=db(tx.query_row("SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",params![domain.id,user],|r|r.get(0)).optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "workspace creation requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &domain.id)?;
        let machine: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_machine_id'",
            [],
            |r| r.get(0),
        ))?;
        let workspace = id("ws")?;
        let created = now();
        db(tx.execute("INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,host_machine_id,created_by_user_id,created_at,updated_at) VALUES(?1,?2,?3,'hosted','user_created',?4,?5,?6,?6)",params![workspace,domain.id,name,machine,user,created]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "workspace.created",
            "workspace",
            &workspace,
            Some(actor),
            serde_json::json!({"domain_id":domain.id,"workspace_id":workspace,"name":name}),
        )?;
        db(tx.commit())?;
        workspace_row(&c, &workspace)
    }
    pub fn archive_workspace(
        &self,
        actor: &PrincipalId,
        domain: Option<&str>,
        workspace: &str,
    ) -> Result<WorkspaceInfo> {
        let info = self.workspace(actor, domain, workspace)?;
        if info.name == "local" {
            return Err(Error::Denied(
                "the reserved local workspace cannot be archived",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let role:Option<String>=db(tx.query_row("SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",params![info.domain_id,user],|r|r.get(0)).optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "workspace archive requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &info.domain_id)?;
        let when = now();
        db(tx.execute("UPDATE workspaces SET archived_at=?1,updated_at=?1,revision=revision+1 WHERE workspace_id=?2 AND archived_at IS NULL",params![when,info.id]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "workspace.archived",
            "workspace",
            &info.id,
            Some(actor),
            serde_json::json!({"domain_id":info.domain_id,"workspace_id":info.id}),
        )?;
        db(tx.commit())?;
        workspace_row(&c, &info.id)
    }
    pub fn local_machine(&self) -> Result<MachineInfo> {
        let c = self.connection()?;
        let id: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='local_machine_id'",
            [],
            |r| r.get(0),
        ))?;
        machine_row(&c, &id)
    }
    pub fn machines(&self, actor: &PrincipalId, domain: Option<&str>) -> Result<Vec<MachineInfo>> {
        let c = self.connection()?;
        let user = current_user(&c, actor)?;
        let mut q=db(c.prepare("SELECT DISTINCT m.machine_id FROM machines m LEFT JOIN domain_memberships dm ON dm.domain_id=m.owning_domain_id WHERE m.kind='local' OR (dm.user_id=?1 AND dm.status='active' AND (?2 IS NULL OR m.owning_domain_id=?2)) ORDER BY m.display_name,m.machine_id"))?;
        let ids = db(
            db(q.query_map(params![user, domain], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        ids.iter().map(|id| machine_row(&c, id)).collect()
    }
    pub fn machine(
        &self,
        actor: &PrincipalId,
        domain: Option<&str>,
        selector: &str,
    ) -> Result<MachineInfo> {
        let c = self.connection()?;
        let user = current_user(&c, actor)?;
        let exact:Option<String>=db(c.query_row("SELECT DISTINCT m.machine_id FROM machines m LEFT JOIN domain_memberships dm ON dm.domain_id=m.owning_domain_id WHERE (m.kind='local' OR (dm.user_id=?1 AND dm.status='active' AND (?2 IS NULL OR m.owning_domain_id=?2))) AND (m.machine_id=?3 OR m.node_id=?3) LIMIT 1",params![user,domain,selector],|r|r.get(0)).optional())?;
        if let Some(id) = exact {
            return machine_row(&c, &id);
        }
        let mut q=db(c.prepare("SELECT DISTINCT m.machine_id FROM machines m LEFT JOIN domain_memberships dm ON dm.domain_id=m.owning_domain_id WHERE (m.kind='local' OR (dm.user_id=?1 AND dm.status='active' AND (?2 IS NULL OR m.owning_domain_id=?2))) AND m.display_name=?3 ORDER BY m.machine_id LIMIT 2"))?;
        let ids = db(
            db(q.query_map(params![user, domain, selector], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        if ids.is_empty() {
            return Err(Error::Denied("machine unavailable or not permitted"));
        }
        if ids.len() > 1 {
            return Err(Error::Conflict(
                "Machine display name is ambiguous; use its machine/node ID",
            ));
        }
        machine_row(&c, &ids[0])
    }
    pub fn register_remote_machine(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        peer_node_id: &str,
        display_name: Option<&str>,
    ) -> Result<MachineInfo> {
        let domain = match domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let peer = self.awnet_peer_by_node_id(peer_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied(
                "remote Machine peer is not currently trusted",
            ));
        }
        let local = self.local_machine()?;
        if local.node_id == peer.node_id {
            return Err(Error::Denied(
                "local AwNode cannot be registered as a remote Machine",
            ));
        }
        let name = display_name.unwrap_or(&peer.display_name);
        label(name, 128)?;

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let role: Option<String> = db(tx
            .query_row(
                "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
                params![domain.id, user],
                |r| r.get(0),
            )
            .optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "remote Machine registration requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &domain.id)?;

        let existing: Option<(String, Option<String>, String, String)> = db(tx
            .query_row(
                "SELECT machine_id,owning_domain_id,kind,status FROM machines WHERE node_id=?1",
                [&peer.node_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional())?;
        let when = now();
        let machine_id = if let Some((machine_id, owning_domain, kind, status)) = existing {
            if kind != "remote" || owning_domain.as_deref() != Some(domain.id.as_str()) {
                return Err(Error::Conflict(
                    "AwNode is already registered as a different Machine authority",
                ));
            }
            if status == "revoked" {
                return Err(Error::Denied(
                    "remote Machine was revoked; automatic re-registration is refused",
                ));
            }
            db(tx.execute(
                "UPDATE machines SET display_name=?1,status='active',last_seen_at=?2 WHERE machine_id=?3",
                params![name, when, machine_id],
            ))?;
            machine_id
        } else {
            let machine_id = id("mach")?;
            let store_id: String = db(tx.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            ))?;
            let fingerprint = format!("sha256:{}", &peer.node_id[5..]);
            db(tx.execute(
                "INSERT INTO machines(machine_id,authority_store_id,owning_domain_id,node_id,display_name,kind,key_fingerprint,status,created_at,last_seen_at) VALUES(?1,?2,?3,?4,?5,'remote',?6,'active',?7,?7)",
                params![
                    machine_id,
                    store_id,
                    domain.id,
                    peer.node_id,
                    name,
                    fingerprint,
                    when
                ],
            ))?;
            machine_id
        };

        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "machine.remote_registered",
            "machine",
            &machine_id,
            Some(actor),
            serde_json::json!({
                "domain_id":domain.id,
                "machine_id":machine_id,
                "peer_id":peer.id,
                "node_id":peer.node_id
            }),
        )?;
        db(tx.commit())?;
        machine_row(&c, &machine_id)
    }

    pub fn create_fleet(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        name: &str,
    ) -> Result<FleetInfo> {
        label(name, 128)?;
        let domain = if let Some(s) = domain_selector {
            self.domain(actor, s)?
        } else {
            self.default_domain(actor)?
        };
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let role:Option<String>=db(tx.query_row("SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",params![domain.id,user],|r|r.get(0)).optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "fleet creation requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &domain.id)?;
        let fleet = id("fleet")?;
        let created = now();
        db(tx.execute("INSERT INTO fleets(fleet_id,domain_id,name,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)",params![fleet,domain.id,name,created]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "fleet.created",
            "fleet",
            &fleet,
            Some(actor),
            serde_json::json!({"domain_id":domain.id,"fleet_id":fleet,"name":name}),
        )?;
        db(tx.commit())?;
        fleet_row(&c, &fleet)
    }
    pub fn fleets(&self, actor: &PrincipalId, domain: Option<&str>) -> Result<Vec<FleetInfo>> {
        let c = self.connection()?;
        let user = current_user(&c, actor)?;
        let mut q=db(c.prepare("SELECT f.fleet_id FROM fleets f JOIN domain_memberships dm ON dm.domain_id=f.domain_id WHERE dm.user_id=?1 AND dm.status='active' AND (?2 IS NULL OR f.domain_id=?2) ORDER BY f.name,f.fleet_id"))?;
        let ids = db(
            db(q.query_map(params![user, domain], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        ids.iter().map(|id| fleet_row(&c, id)).collect()
    }
    pub fn fleet_in_domain(
        &self,
        actor: &PrincipalId,
        domain: Option<&str>,
        selector: &str,
    ) -> Result<FleetInfo> {
        let c = self.connection()?;
        let user = current_user(&c, actor)?;
        let mut q=db(c.prepare("SELECT f.fleet_id FROM fleets f JOIN domain_memberships dm ON dm.domain_id=f.domain_id WHERE dm.user_id=?1 AND dm.status='active' AND (?2 IS NULL OR f.domain_id=?2) AND (f.fleet_id=?3 OR f.name=?3) ORDER BY CASE WHEN f.fleet_id=?3 THEN 0 ELSE 1 END,f.fleet_id LIMIT 2"))?;
        let ids = db(
            db(q.query_map(params![user, domain, selector], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        if ids.is_empty() {
            return Err(Error::Denied("fleet unavailable or not permitted"));
        }
        if ids.len() > 1 && ids[0] != selector {
            return Err(Error::Conflict(
                "Fleet name is ambiguous; specify --domain or opaque ID",
            ));
        }
        fleet_row(&c, &ids[0])
    }
    pub fn fleet(&self, actor: &PrincipalId, selector: &str) -> Result<FleetInfo> {
        self.fleet_in_domain(actor, None, selector)
    }
    pub fn add_fleet_member(
        &self,
        actor: &PrincipalId,
        fleet_selector: &str,
        machine_selector: &str,
    ) -> Result<FleetMemberInfo> {
        self.add_fleet_member_in_domain(actor, None, fleet_selector, machine_selector)
    }
    pub fn add_fleet_member_in_domain(
        &self,
        actor: &PrincipalId,
        domain: Option<&str>,
        fleet_selector: &str,
        machine_selector: &str,
    ) -> Result<FleetMemberInfo> {
        let fleet = self.fleet_in_domain(actor, domain, fleet_selector)?;
        let machine = self.machine(actor, Some(&fleet.domain_id), machine_selector)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let role:Option<String>=db(tx.query_row("SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",params![fleet.domain_id,user],|r|r.get(0)).optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "fleet membership change requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &fleet.domain_id)?;
        let when = now();
        db(tx.execute("INSERT INTO fleet_members(fleet_id,machine_id,state,joined_at) VALUES(?1,?2,'active',?3) ON CONFLICT(fleet_id,machine_id) DO UPDATE SET state='active',removed_at=NULL",params![fleet.id,machine.id,when]))?;
        db(tx.execute("UPDATE fleets SET membership_revision=membership_revision+1,updated_at=?1 WHERE fleet_id=?2",params![when,fleet.id]))?;
        let revision: u64 = db(tx.query_row(
            "SELECT membership_revision FROM fleets WHERE fleet_id=?1",
            [&fleet.id],
            |r| r.get(0),
        ))?;
        db(tx.execute("INSERT INTO fleet_membership_events(event_id,fleet_id,membership_revision,machine_id,action,actor,created_at) VALUES(?1,?2,?3,?4,'added',?5,?6)",params![id("fme")?,fleet.id,revision,machine.id,actor.as_str(),when]))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "fleet.member_added",
            "fleet",
            &fleet.id,
            Some(actor),
            serde_json::json!({"domain_id":fleet.domain_id,"fleet_id":fleet.id,"machine_id":machine.id,"membership_revision":revision}),
        )?;
        db(tx.commit())?;
        Ok(FleetMemberInfo {
            fleet_id: fleet.id,
            machine_id: machine.id,
            state: "active".into(),
            joined_at: when,
            removed_at: None,
        })
    }
    pub fn fleet_members(
        &self,
        actor: &PrincipalId,
        fleet_selector: &str,
    ) -> Result<Vec<FleetMemberInfo>> {
        let fleet = self.fleet(actor, fleet_selector)?;
        let c = self.connection()?;
        let mut q=db(c.prepare("SELECT fleet_id,machine_id,state,joined_at,removed_at FROM fleet_members WHERE fleet_id=?1 AND state<>'removed' ORDER BY machine_id"))?;
        let rows = db(q.query_map([fleet.id], |r| {
            Ok(FleetMemberInfo {
                fleet_id: r.get(0)?,
                machine_id: r.get(1)?,
                state: r.get(2)?,
                joined_at: r.get(3)?,
                removed_at: r.get(4)?,
            })
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())
    }

    pub fn create_fleet_operation(
        &self,
        actor: &PrincipalId,
        fleet_selector: &str,
        source_workspace_selector: Option<&str>,
        operation_id: &str,
        request_sha256: &str,
        execution_policy: &str,
    ) -> Result<FleetOperationSnapshot> {
        label(operation_id, 160)?;
        if request_sha256.len() != 64 || !request_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Config(
                "fleet operation request digest must be 32-byte hex".into(),
            ));
        }
        if !matches!(execution_policy, "all" | "one" | "any_healthy" | "quorum") {
            return Err(Error::Config("unsupported fleet execution policy".into()));
        }
        let fleet = self.fleet(actor, fleet_selector)?;
        let source_workspace = if let Some(selector) = source_workspace_selector {
            Some(self.workspace(actor, Some(&fleet.domain_id), selector)?)
        } else {
            None
        };
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT operation_id FROM fleet_operations WHERE operation_id=?1",
                [operation_id],
                |r| r.get(0),
            )
            .optional())?;
        if existing.is_some() {
            let operation = fleet_operation_row(&tx, operation_id)?;
            if operation.fleet_id != fleet.id
                || operation.source_workspace_id.as_deref()
                    != source_workspace.as_ref().map(|w| w.id.as_str())
                || operation.request_sha256 != request_sha256
                || operation.execution_policy != execution_policy
            {
                return Err(Error::Conflict(
                    "fleet operation ID was reused with different immutable intent",
                ));
            }
            return Ok(FleetOperationSnapshot {
                members: fleet_operation_members_rows(&tx, operation_id)?,
                operation,
                reused: true,
            });
        }
        let user = current_user(&tx, actor)?;
        let permitted: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active')",
            params![fleet.domain_id, user],
            |r| r.get(0),
        ))?;
        if !permitted {
            return Err(Error::Denied("fleet unavailable or not permitted"));
        }
        let mut q = db(tx.prepare(
            "SELECT fm.machine_id FROM fleet_members fm JOIN machines m ON m.machine_id=fm.machine_id WHERE fm.fleet_id=?1 AND fm.state='active' AND m.status='active' ORDER BY fm.machine_id",
        ))?;
        let mut machines = db(db(q.query_map([&fleet.id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        drop(q);
        if machines.is_empty() {
            return Err(Error::Denied("fleet has no active execution members"));
        }
        if execution_policy == "one" {
            machines.truncate(1);
        }
        let created = now();
        db(tx.execute(
            "INSERT INTO fleet_operations(operation_id,fleet_id,source_workspace_id,request_sha256,membership_revision,execution_policy,state,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7,?8,?8)",
            params![
                operation_id,
                fleet.id,
                source_workspace.as_ref().map(|w| w.id.as_str()),
                request_sha256,
                fleet.membership_revision,
                execution_policy,
                actor.as_str(),
                created
            ],
        ))?;
        for (index, machine) in machines.iter().enumerate() {
            db(tx.execute(
                "INSERT INTO fleet_operation_members(operation_id,machine_id,ordinal,state,updated_at) VALUES(?1,?2,?3,'pending',?4)",
                params![operation_id, machine, index as u64 + 1, created],
            ))?;
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "fleet.operation_created",
            "fleet_operation",
            operation_id,
            Some(actor),
            serde_json::json!({
                "fleet_id":fleet.id,
                "membership_revision":fleet.membership_revision,
                "execution_policy":execution_policy,
                "member_count":machines.len(),
                "request_sha256":request_sha256
            }),
        )?;
        db(tx.commit())?;
        Ok(FleetOperationSnapshot {
            operation: fleet_operation_row(&c, operation_id)?,
            members: fleet_operation_members_rows(&c, operation_id)?,
            reused: false,
        })
    }

    pub fn fleet_operation(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
    ) -> Result<FleetOperationSnapshot> {
        let c = self.connection()?;
        let operation = fleet_operation_row(&c, operation_id)?;
        drop(c);
        let fleet = self.fleet(actor, &operation.fleet_id)?;
        if fleet.id != operation.fleet_id {
            return Err(Error::Denied(
                "fleet operation unavailable or not permitted",
            ));
        }
        let c = self.connection()?;
        Ok(FleetOperationSnapshot {
            members: fleet_operation_members_rows(&c, operation_id)?,
            operation,
            reused: true,
        })
    }

    pub fn fleet_operations(
        &self,
        actor: &PrincipalId,
        fleet_selector: &str,
    ) -> Result<Vec<FleetOperationSnapshot>> {
        let fleet = self.fleet(actor, fleet_selector)?;
        let c = self.connection()?;
        let mut q = db(c.prepare(
            "SELECT operation_id FROM fleet_operations WHERE fleet_id=?1 ORDER BY created_at,operation_id",
        ))?;
        let ids = db(db(q.query_map([fleet.id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter()
            .map(|id| {
                Ok(FleetOperationSnapshot {
                    operation: fleet_operation_row(&c, id)?,
                    members: fleet_operation_members_rows(&c, id)?,
                    reused: true,
                })
            })
            .collect()
    }

    pub fn update_fleet_operation_member(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        machine_id: &str,
        state: &str,
        remote_ref_id: Option<&str>,
        result_sha256: Option<&str>,
    ) -> Result<FleetOperationSnapshot> {
        if !matches!(
            state,
            "pending" | "active" | "completed" | "failed" | "unknown" | "skipped"
        ) {
            return Err(Error::Config("invalid fleet member operation state".into()));
        }
        if let Some(digest) = result_sha256 {
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Config(
                    "fleet member result digest must be 32-byte hex".into(),
                ));
            }
        }
        let current = self.fleet_operation(actor, operation_id)?;
        if !current.members.iter().any(|m| m.machine_id == machine_id) {
            return Err(Error::Denied(
                "machine is not part of this pinned fleet operation",
            ));
        }
        if let Some(reference) = remote_ref_id {
            let remote = self.remote_session_ref(actor, reference)?;
            if current.operation.source_workspace_id.as_deref()
                != Some(remote.source_workspace_id.as_str())
            {
                return Err(Error::Denied(
                    "RemoteSessionRef source Workspace differs from fleet operation",
                ));
            }
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let changed = db(tx.execute(
            "UPDATE fleet_operation_members SET state=?1,remote_ref_id=COALESCE(?2,remote_ref_id),result_sha256=COALESCE(?3,result_sha256),updated_at=?4 WHERE operation_id=?5 AND machine_id=?6",
            params![state, remote_ref_id, result_sha256, now(), operation_id, machine_id],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "fleet member operation changed concurrently",
            ));
        }
        let states: Vec<String> = {
            let mut q = db(tx.prepare(
                "SELECT state FROM fleet_operation_members WHERE operation_id=?1 ORDER BY ordinal",
            ))?;
            let rows = db(q.query_map([operation_id], |r| r.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        let aggregate = if states
            .iter()
            .all(|s| matches!(s.as_str(), "completed" | "skipped"))
        {
            "completed"
        } else if states.iter().any(|s| s == "unknown") {
            "unknown"
        } else if states
            .iter()
            .any(|s| matches!(s.as_str(), "completed" | "failed"))
        {
            "partial"
        } else if states.iter().any(|s| s == "active") {
            "running"
        } else {
            "pending"
        };
        db(tx.execute(
            "UPDATE fleet_operations SET state=?1,updated_at=?2 WHERE operation_id=?3",
            params![aggregate, now(), operation_id],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "fleet.member_state",
            "fleet_operation",
            operation_id,
            Some(actor),
            serde_json::json!({
                "machine_id":machine_id,
                "state":state,
                "aggregate_state":aggregate,
                "remote_ref_id":remote_ref_id,
                "result_sha256":result_sha256
            }),
        )?;
        db(tx.commit())?;
        drop(c);
        self.fleet_operation(actor, operation_id)
    }

    pub fn create_fleet_workspace(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        name: &str,
        fleet_selector: &str,
    ) -> Result<WorkspaceInfo> {
        label(name, 128)?;
        let selected_domain = if let Some(d) = domain_selector {
            Some(self.domain(actor, d)?)
        } else {
            None
        };
        let fleet = self.fleet_in_domain(
            actor,
            selected_domain.as_ref().map(|d| d.id.as_str()),
            fleet_selector,
        )?;
        let domain = if let Some(d) = selected_domain {
            d
        } else {
            let c = self.connection()?;
            domain_row(&c, &fleet.domain_id)?
        };
        if domain.id != fleet.domain_id {
            return Err(Error::Denied(
                "fleet and FleetWorkspace must currently belong to the same Domain",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let role:Option<String>=db(tx.query_row("SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",params![domain.id,user],|r|r.get(0)).optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "FleetWorkspace creation requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &domain.id)?;
        let workspace = id("ws")?;
        let created = now();
        db(tx.execute("INSERT INTO workspaces(workspace_id,domain_id,name,kind,origin_kind,policy_json,defaults_json,created_by_user_id,created_at,updated_at) VALUES(?1,?2,?3,'fleet_virtual','fleet_virtual','{}','{}',?4,?5,?5)",params![workspace,domain.id,name,user,created]))?;
        db(tx.execute(
            "INSERT INTO fleet_workspaces(workspace_id,fleet_id) VALUES(?1,?2)",
            params![workspace, fleet.id],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "fleet_workspace.created",
            "workspace",
            &workspace,
            Some(actor),
            serde_json::json!({"domain_id":domain.id,"workspace_id":workspace,"fleet_id":fleet.id}),
        )?;
        db(tx.commit())?;
        workspace_row(&c, &workspace)
    }
    pub fn session_target(
        &self,
        actor: &PrincipalId,
        session: &str,
    ) -> Result<SessionExecutionTarget> {
        let c = self.connection()?;
        authorize(&c, session, actor, false)?;
        session_target_in(&c, session)
    }
    pub fn bind_local_node_identity(
        &self,
        node_id: &str,
        key_fingerprint: &str,
    ) -> Result<MachineInfo> {
        if node_id.len() != 69
            || !node_id.starts_with("node_")
            || !node_id[5..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Config("invalid cryptographic AwNode ID".into()));
        }
        if key_fingerprint.len() != 71
            || !key_fingerprint.starts_with("sha256:")
            || !key_fingerprint[7..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Config("invalid AwNode key fingerprint".into()));
        }
        if node_id[5..] != key_fingerprint[7..] {
            return Err(Error::Denied(
                "AwNode ID and key fingerprint must identify the same public key",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let machine: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_machine_id'",
            [],
            |r| r.get(0),
        ))?;
        let (old_node, old_fingerprint): (String, Option<String>) = db(tx.query_row(
            "SELECT node_id,key_fingerprint FROM machines WHERE machine_id=?1 AND kind='local'",
            [&machine],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        if let Some(existing) = old_fingerprint.as_deref() {
            if existing != key_fingerprint || old_node != node_id {
                return Err(Error::Denied("local AwNode identity does not match the persisted key; restore the original node identity file"));
            }
        }
        if old_fingerprint.is_none() {
            let collision: Option<String> = db(tx
                .query_row(
                    "SELECT machine_id FROM machines WHERE node_id=?1 AND machine_id<>?2",
                    params![node_id, machine],
                    |r| r.get(0),
                )
                .optional())?;
            if collision.is_some() {
                return Err(Error::Conflict(
                    "cryptographic AwNode ID already belongs to another Machine",
                ));
            }
            let when = now();
            db(tx.execute("UPDATE machines SET node_id=?1,key_fingerprint=?2,security_epoch=security_epoch+1,last_seen_at=?3 WHERE machine_id=?4",params![node_id,key_fingerprint,when,machine]))?;
            db(tx.execute(
                "UPDATE store_meta SET value=?1 WHERE key='local_node_id'",
                [node_id],
            ))?;
            authority::replace_local_authority_node(&tx, &old_node, node_id)?;
            crate::workspace_writer::replace_local_writer_node(&tx, &old_node, node_id)?;
            let txid = durability::tx_id()?;
            durability::commit_event(
                &tx,
                &txid,
                "machine.identity_bound",
                "machine",
                &machine,
                None,
                serde_json::json!({"machine_id":machine,"node_id":node_id,"key_fingerprint":key_fingerprint}),
            )?;
        }
        db(tx.commit())?;
        machine_row(&c, &machine)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_domain_federation(
        &self,
        actor: &PrincipalId,
        local_domain_selector: Option<&str>,
        peer_node_id: &str,
        remote_domain_id: &str,
        direction: &str,
        scope: serde_json::Value,
        permissions: serde_json::Value,
        include_child_domains: bool,
        allow_redelegation: bool,
    ) -> Result<DomainFederationInfo> {
        label(remote_domain_id, 160)?;
        if !matches!(direction, "outbound" | "inbound") {
            return Err(Error::Config(
                "federation direction must be outbound or inbound".into(),
            ));
        }
        validate_federation_documents(&scope, &permissions)?;
        let domain = match local_domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let peer = self.awnet_peer_by_node_id(peer_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied("federation peer is not currently trusted"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, actor)?;
        let role: Option<String> = db(tx
            .query_row(
                "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
                params![domain.id, user],
                |r| r.get(0),
            )
            .optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "federation creation requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &domain.id)?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT federation_id FROM domain_federations WHERE local_domain_id=?1 AND remote_peer_id=?2 AND remote_domain_id=?3 AND direction=?4 AND status='active' ORDER BY created_at,federation_id LIMIT 1",
                params![domain.id, peer.id, remote_domain_id, direction],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(id) = existing {
            let current = federation_row(&tx, &id)?;
            if current.scope == scope
                && current.permissions == permissions
                && current.include_child_domains == include_child_domains
                && current.allow_redelegation == allow_redelegation
            {
                return Ok(current);
            }
            return Err(Error::Conflict(
                "an active federation already exists with different policy",
            ));
        }
        let federation = id("fed")?;
        let created = now();
        db(tx.execute(
            "INSERT INTO domain_federations(federation_id,local_domain_id,remote_peer_id,remote_domain_id,direction,scope_json,permissions_json,include_child_domains,allow_redelegation,policy_epoch,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,'active',?10,?10)",
            params![
                federation,
                domain.id,
                peer.id,
                remote_domain_id,
                direction,
                json(&scope)?,
                json(&permissions)?,
                if include_child_domains { 1i64 } else { 0i64 },
                if allow_redelegation { 1i64 } else { 0i64 },
                created
            ],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "federation.created",
            "domain_federation",
            &federation,
            Some(actor),
            serde_json::json!({
                "local_domain_id":domain.id,
                "remote_peer_id":peer.id,
                "remote_domain_id":remote_domain_id,
                "direction":direction,
                "include_child_domains":include_child_domains,
                "allow_redelegation":allow_redelegation
            }),
        )?;
        db(tx.commit())?;
        federation_row(&c, &federation)
    }

    pub fn domain_federations(
        &self,
        actor: &PrincipalId,
        local_domain_selector: Option<&str>,
    ) -> Result<Vec<DomainFederationInfo>> {
        let domain = match local_domain_selector {
            Some(selector) => self.domain(actor, selector)?,
            None => self.default_domain(actor)?,
        };
        let c = self.connection()?;
        let user = current_user(&c, actor)?;
        let permitted: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active')",
            params![domain.id, user],
            |r| r.get(0),
        ))?;
        if !permitted {
            return Err(Error::Denied("Domain unavailable or not permitted"));
        }
        let mut q = db(c.prepare(
            "SELECT federation_id FROM domain_federations WHERE local_domain_id=?1 ORDER BY created_at,federation_id",
        ))?;
        let ids = db(db(q.query_map([domain.id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| federation_row(&c, id)).collect()
    }

    pub fn domain_federation(
        &self,
        actor: &PrincipalId,
        federation_id: &str,
    ) -> Result<DomainFederationInfo> {
        let c = self.connection()?;
        let federation = federation_row(&c, federation_id)?;
        let user = current_user(&c, actor)?;
        let permitted: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active')",
            params![federation.local_domain_id, user],
            |r| r.get(0),
        ))?;
        if !permitted {
            return Err(Error::Denied("federation unavailable or not permitted"));
        }
        Ok(federation)
    }

    pub fn issue_federation_lease(
        &self,
        actor: &PrincipalId,
        federation_id: &str,
        ttl_seconds: u64,
    ) -> Result<FederationLeaseInfo> {
        if !(5..=3600).contains(&ttl_seconds) {
            return Err(Error::Config(
                "federation lease TTL must be between 5 and 3600 seconds".into(),
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let federation = federation_row(&tx, federation_id)?;
        if federation.status != "active" {
            return Err(Error::Denied("federation is not active"));
        }
        let user = current_user(&tx, actor)?;
        let role: Option<String> = db(tx
            .query_row(
                "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
                params![federation.local_domain_id, user],
                |r| r.get(0),
            )
            .optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "federation lease issuance requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &federation.local_domain_id)?;
        let (domain_policy_revision, domain_security_epoch): (u64, u64) = db(tx.query_row(
            "SELECT policy_revision,security_epoch FROM domains WHERE domain_id=?1 AND status='active'",
            [&federation.local_domain_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        let (peer_node, peer_security_epoch, peer_state): (String, u64, String) = db(tx
            .query_row(
                "SELECT node_id,security_epoch,trust_state FROM awnet_peers WHERE peer_id=?1",
                [&federation.remote_peer_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ))?;
        if peer_state != "trusted" {
            return Err(Error::Denied("federation peer trust is revoked"));
        }
        let lease = id("flease")?;
        let created = now();
        let ttl_micros = ttl_seconds
            .checked_mul(1_000_000)
            .and_then(|v| i64::try_from(v).ok())
            .ok_or(Error::Limit("federation lease TTL overflow"))?;
        let expires = created
            .checked_add(ttl_micros)
            .ok_or(Error::Limit("federation lease expiry overflow"))?;
        db(tx.execute(
            "INSERT INTO federation_capability_leases(lease_id,federation_id,remote_peer_id,issued_to_node_id,scope_json,permissions_json,policy_epoch,domain_policy_revision,domain_security_epoch,peer_security_epoch,issued_by,expires_at,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'active',?13)",
            params![
                lease,
                federation.id,
                federation.remote_peer_id,
                peer_node,
                json(&federation.scope)?,
                json(&federation.permissions)?,
                federation.policy_epoch,
                domain_policy_revision,
                domain_security_epoch,
                peer_security_epoch,
                actor.as_str(),
                expires,
                created
            ],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "federation.lease_issued",
            "federation_lease",
            &lease,
            Some(actor),
            serde_json::json!({
                "federation_id":federation.id,
                "issued_to_node_id":peer_node,
                "policy_epoch":federation.policy_epoch,
                "domain_policy_revision":domain_policy_revision,
                "domain_security_epoch":domain_security_epoch,
                "peer_security_epoch":peer_security_epoch,
                "expires_at":expires
            }),
        )?;
        db(tx.commit())?;
        federation_lease_row(&c, &lease)
    }

    pub fn validate_federation_lease(
        &self,
        lease_id: &str,
        peer_node_id: &str,
        action: &str,
    ) -> Result<FederationLeaseInfo> {
        label(action, 128)?;
        let c = self.connection()?;
        let lease = federation_lease_row(&c, lease_id)?;
        if lease.status != "active"
            || lease.expires_at <= now()
            || lease.issued_to_node_id != peer_node_id
        {
            return Err(Error::Denied(
                "federation capability lease is inactive or expired",
            ));
        }
        let federation = federation_row(&c, &lease.federation_id)?;
        let (domain_policy_revision, domain_security_epoch, domain_status): (u64, u64, String) =
            db(c.query_row(
                "SELECT policy_revision,security_epoch,status FROM domains WHERE domain_id=?1",
                [&federation.local_domain_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ))?;
        let (peer_node, peer_security_epoch, peer_state): (String, u64, String) = db(c.query_row(
            "SELECT node_id,security_epoch,trust_state FROM awnet_peers WHERE peer_id=?1",
            [&lease.remote_peer_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ))?;
        if federation.status != "active"
            || domain_status != "active"
            || peer_state != "trusted"
            || peer_node != peer_node_id
            || federation.policy_epoch != lease.policy_epoch
            || domain_policy_revision != lease.domain_policy_revision
            || domain_security_epoch != lease.domain_security_epoch
            || peer_security_epoch != lease.peer_security_epoch
        {
            return Err(Error::Denied(
                "federation capability lease epoch no longer matches current authority",
            ));
        }
        let allowed = lease
            .permissions
            .as_array()
            .is_some_and(|actions| actions.iter().any(|value| value.as_str() == Some(action)));
        if !allowed {
            return Err(Error::Denied(
                "federation capability lease does not permit this action",
            ));
        }
        Ok(lease)
    }

    pub fn revoke_federation_lease(
        &self,
        actor: &PrincipalId,
        lease_id: &str,
    ) -> Result<FederationLeaseInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let lease = federation_lease_row(&tx, lease_id)?;
        let federation = federation_row(&tx, &lease.federation_id)?;
        let user = current_user(&tx, actor)?;
        let role: Option<String> = db(tx
            .query_row(
                "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
                params![federation.local_domain_id, user],
                |r| r.get(0),
            )
            .optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "federation lease revocation requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &federation.local_domain_id)?;
        if lease.status == "active" {
            db(tx.execute(
                "UPDATE federation_capability_leases SET status='revoked',revoked_at=?1 WHERE lease_id=?2 AND status='active'",
                params![now(), lease_id],
            ))?;
            let txid = durability::tx_id()?;
            durability::commit_event(
                &tx,
                &txid,
                "federation.lease_revoked",
                "federation_lease",
                lease_id,
                Some(actor),
                serde_json::json!({"federation_id":federation.id}),
            )?;
        }
        db(tx.commit())?;
        federation_lease_row(&c, lease_id)
    }

    pub fn revoke_domain_federation(
        &self,
        actor: &PrincipalId,
        federation_id: &str,
    ) -> Result<DomainFederationInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let federation = federation_row(&tx, federation_id)?;
        let user = current_user(&tx, actor)?;
        let role: Option<String> = db(tx
            .query_row(
                "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
                params![federation.local_domain_id, user],
                |r| r.get(0),
            )
            .optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "federation revocation requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &federation.local_domain_id)?;
        let revoked = now();
        db(tx.execute(
            "UPDATE domain_federations SET status='revoked',policy_epoch=policy_epoch+1,updated_at=?1 WHERE federation_id=?2 AND status='active'",
            params![revoked, federation_id],
        ))?;
        db(tx.execute(
            "UPDATE federation_capability_leases SET status='revoked',revoked_at=?1 WHERE federation_id=?2 AND status='active'",
            params![revoked, federation_id],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "federation.revoked",
            "domain_federation",
            federation_id,
            Some(actor),
            serde_json::json!({"local_domain_id":federation.local_domain_id}),
        )?;
        db(tx.commit())?;
        federation_row(&c, federation_id)
    }

    pub fn trust_awnet_peer(
        &self,
        node_id: &str,
        public_key: &str,
        display_name: &str,
    ) -> Result<AwNetPeerInfo> {
        label(display_name, 128)?;
        if node_id.len() != 69
            || !node_id.starts_with("node_")
            || !node_id[5..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Config("invalid peer AwNode ID".into()));
        }
        if public_key.len() != 64 || !public_key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Config(
                "peer public key must be a 32-byte hex Ed25519 key".into(),
            ));
        }
        let mut public = [0u8; 32];
        for (index, pair) in public_key.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let pair = std::str::from_utf8(pair)
                .map_err(|_| Error::Config("invalid peer public key".into()))?;
            public[index] = u8::from_str_radix(pair, 16)
                .map_err(|_| Error::Config("invalid peer public key".into()))?;
        }
        if format!("node_{:x}", Sha256::digest(public)) != node_id {
            return Err(Error::Denied("peer Node ID does not match its public key"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let local: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        if local == node_id {
            return Err(Error::Denied("cannot pair an AwNode with itself"));
        }
        let existing: Option<(String, String, String)> = db(tx
            .query_row(
                "SELECT peer_id,public_key,trust_state FROM awnet_peers WHERE node_id=?1",
                [node_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional())?;
        let when = now();
        let peer = if let Some((peer, key, state)) = existing {
            if key != public_key {
                return Err(Error::Denied(
                    "peer Node ID is already bound to a different public key",
                ));
            }
            if state == "revoked" {
                return Err(Error::Denied(
                    "peer trust was revoked; automatic re-trust is refused",
                ));
            }
            db(tx.execute(
                "UPDATE awnet_peers SET display_name=?1,last_seen_at=?2 WHERE peer_id=?3",
                params![display_name, when, peer],
            ))?;
            peer
        } else {
            let peer = id("peer")?;
            db(tx.execute("INSERT INTO awnet_peers(peer_id,node_id,public_key,display_name,trust_state,created_at,last_seen_at) VALUES(?1,?2,?3,?4,'trusted',?5,?5)",params![peer,node_id,public_key,display_name,when]))?;
            peer
        };
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "awnet.peer_trusted",
            "awnet_peer",
            &peer,
            None,
            serde_json::json!({"peer_id":peer,"node_id":node_id,"display_name":display_name}),
        )?;
        db(tx.commit())?;
        db(c.query_row("SELECT peer_id,node_id,public_key,display_name,trust_state,security_epoch,created_at,last_seen_at FROM awnet_peers WHERE peer_id=?1",[peer],|r|Ok(AwNetPeerInfo{id:r.get(0)?,node_id:r.get(1)?,public_key:r.get(2)?,display_name:r.get(3)?,trust_state:r.get(4)?,security_epoch:r.get(5)?,created_at:r.get(6)?,last_seen_at:r.get(7)?})))
    }
    pub fn awnet_peer_by_node_id(&self, node_id: &str) -> Result<AwNetPeerInfo> {
        let c = self.connection()?;
        db(c.query_row(
            "SELECT peer_id,node_id,public_key,display_name,trust_state,security_epoch,created_at,last_seen_at FROM awnet_peers WHERE node_id=?1",
            [node_id],
            |r| Ok(AwNetPeerInfo {
                id: r.get(0)?,
                node_id: r.get(1)?,
                public_key: r.get(2)?,
                display_name: r.get(3)?,
                trust_state: r.get(4)?,
                security_epoch: r.get(5)?,
                created_at: r.get(6)?,
                last_seen_at: r.get(7)?,
            }),
        ))
        .map_err(|_| Error::Denied("AwNet peer is not paired or no longer available"))
    }

    pub fn awnet_peers(&self) -> Result<Vec<AwNetPeerInfo>> {
        let c = self.connection()?;
        let mut q=db(c.prepare("SELECT peer_id,node_id,public_key,display_name,trust_state,security_epoch,created_at,last_seen_at FROM awnet_peers WHERE trust_state='trusted' ORDER BY display_name,peer_id"))?;
        let rows = db(q.query_map([], |r| {
            Ok(AwNetPeerInfo {
                id: r.get(0)?,
                node_id: r.get(1)?,
                public_key: r.get(2)?,
                display_name: r.get(3)?,
                trust_state: r.get(4)?,
                security_epoch: r.get(5)?,
                created_at: r.get(6)?,
                last_seen_at: r.get(7)?,
            })
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())
    }
    pub fn node_status_with_transport(&self, transport: &str) -> Result<serde_json::Value> {
        let c = self.connection()?;
        let store: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))?;
        let machine: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='local_machine_id'",
            [],
            |r| r.get(0),
        ))?;
        let (node, key_fingerprint): (String, Option<String>) = db(c.query_row(
            "SELECT node_id,key_fingerprint FROM machines WHERE machine_id=?1",
            [&machine],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        let peers: i64 = db(c.query_row(
            "SELECT count(*) FROM awnet_peers WHERE trust_state='trusted'",
            [],
            |r| r.get(0),
        ))?;
        let schema_epoch: i64 = db(c.query_row(
            "SELECT schema_epoch FROM schema_meta WHERE singleton=1",
            [],
            |r| r.get(0),
        ))?;
        let schema_version: i64 = db(c.pragma_query_value(None, "user_version", |r| r.get(0)))?;
        Ok(serde_json::json!({
            "store_id":store,
            "machine_id":machine,
            "node_id":node,
            "key_fingerprint":key_fingerprint,
            "trusted_peers":peers,
            "transport":transport,
            "schema_epoch":schema_epoch,
            "schema_version":schema_version
        }))
    }
    pub fn node_status(&self) -> Result<serde_json::Value> {
        self.node_status_with_transport("not_enabled")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn authorize_remote_session_peer(
        &self,
        actor: &PrincipalId,
        session_id: &str,
        source_node_id: &str,
    ) -> Result<SessionInfo> {
        let c = self.connection()?;
        authorize(&c, session_id, actor, false)?;
        let origin: Option<String> = db(c
            .query_row(
                "SELECT source_node_id FROM remote_session_origins WHERE session_id=?1",
                [session_id],
                |r| r.get(0),
            )
            .optional())?;
        if origin.as_deref() != Some(source_node_id) {
            return Err(Error::Denied(
                "remote Session is not delegated from this authenticated peer",
            ));
        }
        drop(c);
        self.session(actor, session_id)
    }

    pub fn begin_remote_session_execution(
        &self,
        actor: &PrincipalId,
        source_node_id: &str,
        session_id: &str,
        branch_id: &str,
        execution_id: &str,
        prompt_sha256: &str,
        owner_request: serde_json::Value,
    ) -> Result<(RemoteSessionExecutionInfo, bool)> {
        if execution_id.is_empty()
            || execution_id.len() > 64
            || !execution_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || prompt_sha256.len() != 64
            || !prompt_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Config(
                "invalid remote Session execution identity".into(),
            ));
        }
        self.authorize_remote_session_peer(actor, session_id, source_node_id)?;
        let branches = self.branches(actor, session_id)?;
        if !branches.iter().any(|b| b.id == branch_id) {
            return Err(Error::Denied("remote Session branch is unavailable"));
        }
        let encoded = json(&owner_request)?;
        if encoded.len() > 512 * 1024 {
            return Err(Error::Limit("remote Session owner request exceeds 512 KiB"));
        }

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let existing: Option<(String, String, String, String, String)> = db(tx
            .query_row(
                "SELECT session_id,branch_id,prompt_sha256,owner_request_json,state FROM remote_session_executions WHERE source_node_id=?1 AND execution_id=?2",
                params![source_node_id, execution_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional())?;
        if let Some((session, branch, prompt, _request, _state)) = existing {
            if session != session_id || branch != branch_id || prompt != prompt_sha256 {
                return Err(Error::Conflict(
                    "remote execution ID was reused for different input",
                ));
            }
            drop(tx);
            return Ok((
                remote_session_execution_row(&c, source_node_id, execution_id)?,
                true,
            ));
        }
        let created = now();
        db(tx.execute(
            "INSERT INTO remote_session_executions(source_node_id,execution_id,session_id,branch_id,prompt_sha256,owner_request_json,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'prepared',?7,?7)",
            params![source_node_id, execution_id, session_id, branch_id, prompt_sha256, encoded, created],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "remote_session.execution_prepared",
            "session",
            session_id,
            Some(actor),
            serde_json::json!({
                "session_id":session_id,
                "branch_id":branch_id,
                "source_node_id":source_node_id,
                "execution_id":execution_id,
                "prompt_sha256":prompt_sha256
            }),
        )?;
        db(tx.commit())?;
        Ok((
            remote_session_execution_row(&c, source_node_id, execution_id)?,
            false,
        ))
    }

    pub fn mark_remote_session_execution_unknown(
        &self,
        actor: &PrincipalId,
        source_node_id: &str,
        execution_id: &str,
    ) -> Result<RemoteSessionExecutionInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = remote_session_execution_row(&tx, source_node_id, execution_id)?;
        authorize(&tx, &current.session_id, actor, false)?;
        if current.state == "prepared" {
            db(tx.execute(
                "UPDATE remote_session_executions SET state='unknown',updated_at=?1 WHERE source_node_id=?2 AND execution_id=?3",
                params![now(), source_node_id, execution_id],
            ))?;
        }
        db(tx.commit())?;
        remote_session_execution_row(&c, source_node_id, execution_id)
    }

    pub fn settle_remote_session_execution(
        &self,
        actor: &PrincipalId,
        source_node_id: &str,
        execution_id: &str,
        run_id: u64,
        state: &str,
    ) -> Result<RemoteSessionExecutionInfo> {
        if run_id == 0 || !matches!(state, "active" | "settled" | "cancelled") {
            return Err(Error::Config(
                "invalid remote Session execution settlement".into(),
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = remote_session_execution_row(&tx, source_node_id, execution_id)?;
        authorize(&tx, &current.session_id, actor, false)?;
        if let Some(existing) = current.run_id {
            if existing != run_id {
                return Err(Error::Conflict("remote execution run identity changed"));
            }
        }
        db(tx.execute(
            "UPDATE remote_session_executions SET state=?1,run_id=?2,updated_at=?3 WHERE source_node_id=?4 AND execution_id=?5",
            params![state, run_id, now(), source_node_id, execution_id],
        ))?;
        db(tx.commit())?;
        remote_session_execution_row(&c, source_node_id, execution_id)
    }

    pub fn remote_session_execution_optional(
        &self,
        actor: &PrincipalId,
        source_node_id: &str,
        execution_id: &str,
    ) -> Result<Option<RemoteSessionExecutionInfo>> {
        let c = self.connection()?;
        let exists: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM remote_session_executions WHERE source_node_id=?1 AND execution_id=?2)",
            params![source_node_id, execution_id],
            |r| r.get(0),
        ))?;
        if !exists {
            return Ok(None);
        }
        let info = remote_session_execution_row(&c, source_node_id, execution_id)?;
        authorize(&c, &info.session_id, actor, false)?;
        Ok(Some(info))
    }

    pub fn remote_session_execution(
        &self,
        actor: &PrincipalId,
        source_node_id: &str,
        execution_id: &str,
    ) -> Result<RemoteSessionExecutionInfo> {
        let c = self.connection()?;
        let info = remote_session_execution_row(&c, source_node_id, execution_id)?;
        authorize(&c, &info.session_id, actor, false)?;
        Ok(info)
    }

    pub fn create_peer_session_idempotent(
        &self,
        actor: &PrincipalId,
        source_store_id: &str,
        source_domain_id: &str,
        source_workspace_id: &str,
        source_workspace_name: &str,
        source_node_id: &str,
        source_session_id: Option<&str>,
        source_branch_id: Option<&str>,
        operation_id: &str,
        request_sha256: &str,
        title: &str,
        binding: SessionBinding,
    ) -> Result<PeerSessionPlacement> {
        for value in [
            source_store_id,
            source_domain_id,
            source_workspace_id,
            source_node_id,
            operation_id,
        ] {
            label(value, 160)?;
        }
        label(source_workspace_name, 128)?;
        label(title, 160)?;
        if source_session_id.is_some() != source_branch_id.is_some() {
            return Err(Error::Config(
                "source Session and branch provenance must be supplied together".into(),
            ));
        }
        if let Some(value) = source_session_id {
            label(value, 160)?;
        }
        if let Some(value) = source_branch_id {
            label(value, 160)?;
        }
        if request_sha256.len() != 64 || !request_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Config(
                "remote Session request digest must be 32-byte hex".into(),
            ));
        }
        validate_binding(&binding)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let existing: Option<RemoteSessionOriginRow> = db(tx
            .query_row(
                "SELECT session_id,source_domain_id,source_workspace_id,source_session_id,source_branch_id,request_sha256,source_node_id FROM remote_session_origins WHERE source_store_id=?1 AND source_operation_id=?2",
                params![source_store_id, operation_id],
                |r| Ok(RemoteSessionOriginRow {
                    session_id: r.get(0)?,
                    source_domain_id: r.get(1)?,
                    source_workspace_id: r.get(2)?,
                    source_session_id: r.get(3)?,
                    source_branch_id: r.get(4)?,
                    request_sha256: r.get(5)?,
                    source_node_id: r.get(6)?,
                }),
            )
            .optional())?;
        if let Some(existing) = existing {
            if existing.source_domain_id != source_domain_id
                || existing.source_workspace_id != source_workspace_id
                || existing.source_session_id.as_deref() != source_session_id
                || existing.source_branch_id.as_deref() != source_branch_id
                || existing.request_sha256 != request_sha256
                || existing.source_node_id != source_node_id
            {
                return Err(Error::Conflict(
                    "remote Session operation ID was reused with different immutable intent",
                ));
            }
            let session = session_in(&tx, &existing.session_id)?;
            if session.owner != *actor {
                return Err(Error::Denied(
                    "remote Session operation belongs to another local principal",
                ));
            }
            let branch = branch_in(&tx, &existing.session_id, "main")?;
            let workspace = workspace_row(&tx, &session.workspace_id)?;
            return Ok(PeerSessionPlacement {
                workspace,
                session,
                branch,
                reused: true,
            });
        }

        let (workspace, workspace_created) = ensure_peer_workspace_tx(
            &tx,
            actor,
            source_store_id,
            source_domain_id,
            source_workspace_id,
            source_workspace_name,
            source_node_id,
        )?;
        let (session, branch) =
            create_in_workspace_tx(&tx, actor, title, &binding, &workspace.id, None)?;
        let created = now();
        db(tx.execute(
            "INSERT INTO remote_session_origins(session_id,workspace_id,source_store_id,source_domain_id,source_workspace_id,source_session_id,source_branch_id,source_operation_id,request_sha256,source_node_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                session.id,
                workspace.id,
                source_store_id,
                source_domain_id,
                source_workspace_id,
                source_session_id,
                source_branch_id,
                operation_id,
                request_sha256,
                source_node_id,
                created
            ],
        ))?;
        let txid = durability::tx_id()?;
        if workspace_created {
            durability::commit_event(
                &tx,
                &txid,
                "peer_workspace.created",
                "workspace",
                &workspace.id,
                Some(actor),
                serde_json::json!({
                    "workspace_id":workspace.id,
                    "source_store_id":source_store_id,
                    "source_domain_id":source_domain_id,
                    "source_workspace_id":source_workspace_id,
                    "source_node_id":source_node_id
                }),
            )?;
        }
        durability::commit_event(
            &tx,
            &txid,
            "remote_session.created",
            "session",
            &session.id,
            Some(actor),
            serde_json::json!({
                "session_id":session.id,
                "workspace_id":workspace.id,
                "source_operation_id":operation_id,
                "request_sha256":request_sha256
            }),
        )?;
        db(tx.commit())?;
        Ok(PeerSessionPlacement {
            workspace,
            session,
            branch,
            reused: false,
        })
    }

    pub fn begin_remote_session_ref(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        request_sha256: &str,
        source_workspace_id: &str,
        source_session_id: Option<&str>,
        peer_node_id: &str,
    ) -> Result<RemoteSessionRef> {
        for value in [operation_id, source_workspace_id, peer_node_id] {
            label(value, 160)?;
        }
        if request_sha256.len() != 64 || !request_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Config(
                "remote Session request digest must be 32-byte hex".into(),
            ));
        }
        if let Some(value) = source_session_id {
            label(value, 160)?;
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user_with_default(&tx, actor)?;
        let permitted: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspaces w JOIN domain_memberships dm ON dm.domain_id=w.domain_id WHERE w.workspace_id=?1 AND w.archived_at IS NULL AND dm.user_id=?2 AND dm.status='active')",
            params![source_workspace_id, user],
            |r| r.get(0),
        ))?;
        if !permitted {
            return Err(Error::Denied(
                "source Workspace unavailable or not permitted",
            ));
        }
        if let Some(session_id) = source_session_id {
            authorize(&tx, session_id, actor, false)?;
            if session_in(&tx, session_id)?.workspace_id != source_workspace_id {
                return Err(Error::Denied(
                    "source Session does not belong to the selected source Workspace",
                ));
            }
        }
        let peer_id: Option<String> = db(tx
            .query_row(
                "SELECT peer_id FROM awnet_peers WHERE node_id=?1 AND trust_state='trusted'",
                [peer_node_id],
                |r| r.get(0),
            )
            .optional())?;
        let peer_id = peer_id.ok_or(Error::Denied(
            "remote Session target is not a currently trusted AwNet peer",
        ))?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT ref_id FROM remote_session_refs WHERE operation_id=?1",
                [operation_id],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(existing) = existing {
            let value = remote_session_ref_row(&tx, &existing)?;
            if value.request_sha256 != request_sha256
                || value.source_workspace_id != source_workspace_id
                || value.source_session_id.as_deref() != source_session_id
                || value.peer_node_id != peer_node_id
            {
                return Err(Error::Conflict(
                    "remote Session operation ID was reused with different immutable intent",
                ));
            }
            return Ok(value);
        }
        let ref_id = id("rr")?;
        let created = now();
        db(tx.execute(
            "INSERT INTO remote_session_refs(ref_id,operation_id,request_sha256,source_workspace_id,source_session_id,remote_peer_id,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7,?7)",
            params![
                ref_id,
                operation_id,
                request_sha256,
                source_workspace_id,
                source_session_id,
                peer_id,
                created
            ],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "remote_session.intent",
            "remote_session_ref",
            &ref_id,
            Some(actor),
            serde_json::json!({
                "operation_id":operation_id,
                "request_sha256":request_sha256,
                "source_workspace_id":source_workspace_id,
                "peer_node_id":peer_node_id
            }),
        )?;
        db(tx.commit())?;
        remote_session_ref_row(&c, &ref_id)
    }

    pub fn mark_remote_session_ref_unknown(
        &self,
        actor: &PrincipalId,
        selector: &str,
    ) -> Result<RemoteSessionRef> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = authorized_remote_session_ref(&tx, actor, selector)?;
        if current.state == "pending" {
            db(tx.execute(
                "UPDATE remote_session_refs SET state='unknown',updated_at=?1 WHERE ref_id=?2 AND state='pending'",
                params![now(), current.id],
            ))?;
            let txid = durability::tx_id()?;
            durability::commit_event(
                &tx,
                &txid,
                "remote_session.unknown",
                "remote_session_ref",
                &current.id,
                Some(actor),
                serde_json::json!({"operation_id":current.operation_id}),
            )?;
        }
        db(tx.commit())?;
        remote_session_ref_row(&c, &current.id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resolve_remote_session_ref(
        &self,
        actor: &PrincipalId,
        selector: &str,
        remote_store_id: &str,
        remote_workspace_id: &str,
        remote_session_id: &str,
        remote_branch_id: &str,
    ) -> Result<RemoteSessionRef> {
        for value in [
            remote_store_id,
            remote_workspace_id,
            remote_session_id,
            remote_branch_id,
        ] {
            label(value, 160)?;
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = authorized_remote_session_ref(&tx, actor, selector)?;
        if matches!(current.state.as_str(), "active" | "completed" | "revoked") {
            if current.remote_store_id.as_deref() == Some(remote_store_id)
                && current.remote_workspace_id.as_deref() == Some(remote_workspace_id)
                && current.remote_session_id.as_deref() == Some(remote_session_id)
                && current.remote_branch_id.as_deref() == Some(remote_branch_id)
            {
                return Ok(current);
            }
            return Err(Error::Conflict(
                "resolved RemoteSessionRef cannot be retargeted to another remote Session",
            ));
        }
        db(tx.execute(
            "UPDATE remote_session_refs SET remote_store_id=?1,remote_workspace_id=?2,remote_session_id=?3,remote_branch_id=?4,state='active',updated_at=?5 WHERE ref_id=?6 AND state IN ('pending','unknown')",
            params![
                remote_store_id,
                remote_workspace_id,
                remote_session_id,
                remote_branch_id,
                now(),
                current.id
            ],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "remote_session.resolved",
            "remote_session_ref",
            &current.id,
            Some(actor),
            serde_json::json!({
                "operation_id":current.operation_id,
                "remote_store_id":remote_store_id,
                "remote_workspace_id":remote_workspace_id,
                "remote_session_id":remote_session_id,
                "remote_branch_id":remote_branch_id
            }),
        )?;
        db(tx.commit())?;
        remote_session_ref_row(&c, &current.id)
    }

    pub fn remote_session_ref(
        &self,
        actor: &PrincipalId,
        selector: &str,
    ) -> Result<RemoteSessionRef> {
        let c = self.connection()?;
        authorized_remote_session_ref(&c, actor, selector)
    }

    pub fn remote_session_refs(&self, actor: &PrincipalId) -> Result<Vec<RemoteSessionRef>> {
        let c = self.connection()?;
        let user = current_user_with_default(&c, actor)?;
        let mut query = db(c.prepare(
            "SELECT r.ref_id FROM remote_session_refs r JOIN workspaces w ON w.workspace_id=r.source_workspace_id JOIN domain_memberships dm ON dm.domain_id=w.domain_id WHERE dm.user_id=?1 AND dm.status='active' ORDER BY r.created_at,r.ref_id",
        ))?;
        let ids = db(db(query.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter()
            .map(|id| remote_session_ref_row(&c, id))
            .collect()
    }

    pub fn create_in_workspace(
        &self,
        owner: &PrincipalId,
        title: &str,
        binding: SessionBinding,
        domain_selector: Option<&str>,
        workspace_selector: Option<&str>,
        target: Option<SessionExecutionTarget>,
    ) -> Result<(SessionInfo, BranchInfo)> {
        label(title, 160)?;
        validate_binding(&binding)?;
        let selected_domain = if let Some(selector) = domain_selector {
            Some(self.domain(owner, selector)?.id)
        } else {
            None
        };
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = current_user(&tx, owner)?;
        let realm: String = db(tx.query_row(
            "SELECT realm_id FROM users WHERE user_id=?1",
            [&user],
            |r| r.get(0),
        ))?;
        let (_, default_local_ws) = ensure_default_domain_workspace(&tx, &user, &realm)?;
        let workspace = if let Some(selector) = workspace_selector {
            let mut q=db(tx.prepare("SELECT w.workspace_id FROM workspaces w JOIN domain_memberships dm ON dm.domain_id=w.domain_id WHERE dm.user_id=?1 AND dm.status='active' AND w.archived_at IS NULL AND (?2 IS NULL OR w.domain_id=?2) AND (w.workspace_id=?3 OR w.name=?3) ORDER BY CASE WHEN w.workspace_id=?3 THEN 0 ELSE 1 END,w.workspace_id LIMIT 2"))?;
            let ids = db(
                db(q.query_map(params![user, selected_domain, selector], |r| {
                    r.get::<_, String>(0)
                }))?
                .collect::<rusqlite::Result<Vec<_>>>(),
            )?;
            if ids.is_empty() {
                return Err(Error::Denied("workspace unavailable or not permitted"));
            }
            if ids.len() > 1 && ids[0] != selector {
                return Err(Error::Conflict(
                    "Workspace name is ambiguous; specify --domain or opaque ID",
                ));
            }
            ids[0].clone()
        } else if let Some(domain) = selected_domain {
            db(tx.query_row("SELECT workspace_id FROM workspaces WHERE domain_id=?1 AND name='local' AND archived_at IS NULL",[domain],|r|r.get(0)))?
        } else {
            default_local_ws
        };
        let result = create_in_workspace_tx(&tx, owner, title, &binding, &workspace, target)?;
        db(tx.commit())?;
        Ok(result)
    }
}

pub(crate) fn create_in_workspace_tx(
    c: &Connection,
    owner: &PrincipalId,
    title: &str,
    b: &SessionBinding,
    workspace: &str,
    target: Option<SessionExecutionTarget>,
) -> Result<(SessionInfo, BranchInfo)> {
    let user = ensure_user_for_principal(c, owner)?;
    let allowed:Option<(String,String)>=db(c.query_row("SELECT w.domain_id,w.kind FROM workspaces w JOIN domain_memberships dm ON dm.domain_id=w.domain_id WHERE w.workspace_id=?1 AND w.archived_at IS NULL AND dm.user_id=?2 AND dm.status='active'",params![workspace,user],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
    let Some((workspace_domain, workspace_kind)) = allowed else {
        return Err(Error::Denied("workspace unavailable or not permitted"));
    };
    let _writer = crate::workspace_writer::require_workspace_writer(c, workspace)?;
    let session = id("s")?;
    let root = id("n")?;
    let branch = id("b")?;
    let created = now();
    db(c.execute("INSERT INTO sessions(id,owner,title,binding_json,root_key,root_node,created_at,updated_at,owner_user_id,workspace_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?7,?8,?9)",params![session,owner.as_str(),title,json(b)?,key(b)?,root,created,user,workspace]))?;
    insert_node(
        c,
        &session,
        &root,
        None,
        "root",
        owner,
        serde_json::json!({"format":"aw.session","version":1}),
    )?;
    db(c.execute(
        "INSERT INTO branches(id,session_id,name,head,created_at) VALUES(?1,?2,'main',?3,?4)",
        params![branch, session, root, created],
    ))?;
    let local_machine: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='local_machine_id'",
        [],
        |r| r.get(0),
    ))?;
    let target = if let Some(target) = target {
        target
    } else if workspace_kind == "fleet_virtual" {
        let (fleet,revision):(String,u64)=db(c.query_row("SELECT fw.fleet_id,f.membership_revision FROM fleet_workspaces fw JOIN fleets f ON f.fleet_id=fw.fleet_id WHERE fw.workspace_id=?1",[workspace],|r|Ok((r.get(0)?,r.get(1)?))))?;
        SessionExecutionTarget::Fleet {
            fleet_id: fleet,
            membership_revision: Some(revision),
        }
    } else {
        SessionExecutionTarget::Local {
            machine_id: local_machine.clone(),
        }
    };
    match &target {
        SessionExecutionTarget::Local { machine_id } => {
            if machine_id != &local_machine {
                return Err(Error::Denied(
                    "local target must reference this AwNode's local Machine",
                ));
            }
            db(c.execute("INSERT INTO session_execution_targets(session_id,target_kind,machine_id,target_snapshot_json,updated_at) VALUES(?1,'local',?2,'{}',?3)",params![session,machine_id,created]))?;
        }
        SessionExecutionTarget::Machine { machine_id } => {
            let machine:Option<(Option<String>,String)>=db(c.query_row("SELECT owning_domain_id,kind FROM machines WHERE machine_id=?1 AND status<>'revoked'",[machine_id],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
            let Some((owning_domain, kind)) = machine else {
                return Err(Error::Denied("target machine unavailable"));
            };
            if kind != "local" && owning_domain.as_deref() != Some(workspace_domain.as_str()) {
                return Err(Error::Denied(
                    "target Machine is outside the Session Workspace Domain",
                ));
            }
            db(c.execute("INSERT INTO session_execution_targets(session_id,target_kind,machine_id,target_snapshot_json,updated_at) VALUES(?1,'machine',?2,'{}',?3)",params![session,machine_id,created]))?;
        }
        SessionExecutionTarget::Fleet {
            fleet_id,
            membership_revision,
        } => {
            let fleet: Option<(String, u64)> = db(c
                .query_row(
                    "SELECT domain_id,membership_revision FROM fleets WHERE fleet_id=?1",
                    [fleet_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional())?;
            let Some((fleet_domain, current_revision)) = fleet else {
                return Err(Error::Denied("target fleet unavailable"));
            };
            if fleet_domain != workspace_domain {
                return Err(Error::Denied(
                    "target Fleet is outside the Session Workspace Domain",
                ));
            }
            if workspace_kind == "fleet_virtual" {
                let expected: String = db(c.query_row(
                    "SELECT fleet_id FROM fleet_workspaces WHERE workspace_id=?1",
                    [workspace],
                    |r| r.get(0),
                ))?;
                if expected.as_str() != fleet_id {
                    return Err(Error::Denied(
                        "FleetWorkspace Sessions must target their bound Fleet",
                    ));
                }
            }
            let revision = membership_revision.unwrap_or(current_revision);
            if revision == 0 || revision > current_revision {
                return Err(Error::Conflict("invalid future Fleet membership revision"));
            }
            db(c.execute("INSERT INTO session_execution_targets(session_id,target_kind,fleet_id,target_snapshot_json,updated_at) VALUES(?1,'fleet',?2,?3,?4)",params![session,fleet_id,json(&serde_json::json!({"membership_revision":revision}))?,created]))?;
        }
    }
    let txid = durability::tx_id()?;
    durability::commit_event(
        c,
        &txid,
        "session.created",
        "session",
        &session,
        Some(owner),
        serde_json::json!({"session_id":session,"branch_id":branch,"head":root,"workspace_id":workspace,"target":target}),
    )?;
    Ok((session_in(c, &session)?, branch_in(c, &session, &branch)?))
}
