use crate::{
    db, id, json, now, stable_user_principal, user_for_principal, AuthenticatedStableUser,
    SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_TTL_SECS: u64 = 60 * 60;
const MIN_TTL_SECS: u64 = 5;
const MAX_ACTIVE_GRANTS: i64 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteViewPermission {
    Files,
    Git,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteViewGrantState {
    Active,
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteViewGrant {
    pub grant_id: String,
    pub domain_id: String,
    pub workspace_id: String,
    pub workspace_revision: u64,
    pub workspace_policy_revision: u64,

    pub target_machine_id: String,
    pub target_machine_node_id: String,
    pub target_machine_security_epoch: u64,
    pub target_machine_capability_revision: u64,

    pub requester_machine_id: String,
    pub requester_machine_node_id: String,
    pub requester_machine_security_epoch: u64,
    pub requester_machine_capability_revision: u64,

    pub peer_id: String,
    pub peer_node_id: String,
    pub peer_security_epoch: u64,
    pub peer_public_key: String,

    pub user_id: String,
    pub stable_principal: String,
    pub user_security_epoch: u64,
    pub domain_policy_revision: u64,
    pub domain_security_epoch: u64,

    pub device_binding_id: String,
    pub device_id: String,
    pub device_public_key: String,

    pub permissions: BTreeSet<RemoteViewPermission>,
    pub state: RemoteViewGrantState,
    pub revision: u64,
    pub issued_at: i64,
    pub expires_at: i64,
    pub updated_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug)]
struct PeerFence {
    id: String,
    node_id: String,
    public_key: String,
    security_epoch: u64,
}

#[derive(Clone, Debug)]
struct MachineFence {
    id: String,
    node_id: String,
    kind: String,
    status: String,
    owning_domain_id: Option<String>,
    capability_revision: u64,
    security_epoch: u64,
}

#[derive(Clone, Debug)]
struct WorkspaceFence {
    domain_id: String,
    kind: String,
    host_machine_id: Option<String>,
    policy_revision: u64,
    revision: u64,
    archived: bool,
}

#[derive(Clone, Debug)]
struct DeviceFence {
    binding_id: String,
    device_id: String,
    public_key: String,
    domain_policy_revision: u64,
}

fn state(value: &str) -> Result<RemoteViewGrantState> {
    match value {
        "active" => Ok(RemoteViewGrantState::Active),
        "revoked" => Ok(RemoteViewGrantState::Revoked),
        _ => Err(Error::Storage("invalid remote-view grant state")),
    }
}

fn grant_row(c: &Connection, grant_id: &str) -> Result<RemoteViewGrant> {
    let row = db(c
        .query_row(
            "SELECT grant_id,domain_id,workspace_id,workspace_revision,workspace_policy_revision,
                    target_machine_id,target_machine_node_id,target_machine_security_epoch,target_machine_capability_revision,
                    requester_machine_id,requester_machine_node_id,requester_machine_security_epoch,requester_machine_capability_revision,
                    peer_id,peer_node_id,peer_security_epoch,peer_public_key,
                    user_id,stable_principal,user_security_epoch,domain_policy_revision,domain_security_epoch,
                    device_binding_id,device_id,device_public_key,permissions_json,status,revision,
                    issued_at,expires_at,updated_at,revoked_at
             FROM remote_view_grants WHERE grant_id=?1",
            [grant_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, u64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, u64>(7)?,
                    r.get::<_, u64>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, String>(10)?,
                    r.get::<_, u64>(11)?,
                    r.get::<_, u64>(12)?,
                    r.get::<_, String>(13)?,
                    r.get::<_, String>(14)?,
                    r.get::<_, u64>(15)?,
                    r.get::<_, String>(16)?,
                    r.get::<_, String>(17)?,
                    r.get::<_, String>(18)?,
                    r.get::<_, u64>(19)?,
                    r.get::<_, u64>(20)?,
                    r.get::<_, u64>(21)?,
                    r.get::<_, String>(22)?,
                    r.get::<_, String>(23)?,
                    r.get::<_, String>(24)?,
                    r.get::<_, String>(25)?,
                    r.get::<_, String>(26)?,
                    r.get::<_, u64>(27)?,
                    r.get::<_, i64>(28)?,
                    r.get::<_, i64>(29)?,
                    r.get::<_, i64>(30)?,
                    r.get::<_, Option<i64>>(31)?,
                ))
            },
        )
        .optional())?
        .ok_or(Error::Denied("remote-view grant unavailable"))?;
    let permissions = serde_json::from_str(&row.25)
        .map_err(|_| Error::Storage("invalid remote-view permission set"))?;
    Ok(RemoteViewGrant {
        grant_id: row.0,
        domain_id: row.1,
        workspace_id: row.2,
        workspace_revision: row.3,
        workspace_policy_revision: row.4,
        target_machine_id: row.5,
        target_machine_node_id: row.6,
        target_machine_security_epoch: row.7,
        target_machine_capability_revision: row.8,
        requester_machine_id: row.9,
        requester_machine_node_id: row.10,
        requester_machine_security_epoch: row.11,
        requester_machine_capability_revision: row.12,
        peer_id: row.13,
        peer_node_id: row.14,
        peer_security_epoch: row.15,
        peer_public_key: row.16,
        user_id: row.17,
        stable_principal: row.18,
        user_security_epoch: row.19,
        domain_policy_revision: row.20,
        domain_security_epoch: row.21,
        device_binding_id: row.22,
        device_id: row.23,
        device_public_key: row.24,
        permissions,
        state: state(&row.26)?,
        revision: row.27,
        issued_at: row.28,
        expires_at: row.29,
        updated_at: row.30,
        revoked_at: row.31,
    })
}

fn peer_fence(c: &Connection, node_id: &str) -> Result<PeerFence> {
    db(c.query_row(
        "SELECT peer_id,node_id,public_key,security_epoch,trust_state
             FROM awnet_peers WHERE node_id=?1",
        [node_id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u64>(3)?,
                r.get::<_, String>(4)?,
            ))
        },
    )
    .optional())?
    .and_then(|row| {
        (row.4 == "trusted").then_some(PeerFence {
            id: row.0,
            node_id: row.1,
            public_key: row.2,
            security_epoch: row.3,
        })
    })
    .ok_or(Error::Denied("remote-view peer is not currently trusted"))
}

fn machine_by_node(c: &Connection, node_id: &str) -> Result<MachineFence> {
    db(c.query_row(
        "SELECT machine_id,node_id,kind,status,owning_domain_id,capability_revision,security_epoch
             FROM machines WHERE node_id=?1",
        [node_id],
        |r| {
            Ok(MachineFence {
                id: r.get(0)?,
                node_id: r.get(1)?,
                kind: r.get(2)?,
                status: r.get(3)?,
                owning_domain_id: r.get(4)?,
                capability_revision: r.get(5)?,
                security_epoch: r.get(6)?,
            })
        },
    )
    .optional())?
    .ok_or(Error::Denied("remote-view Machine is not registered"))
}

fn local_machine(c: &Connection) -> Result<MachineFence> {
    let id: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='local_machine_id'",
        [],
        |r| r.get(0),
    ))?;
    db(c.query_row(
        "SELECT machine_id,node_id,kind,status,owning_domain_id,capability_revision,security_epoch
         FROM machines WHERE machine_id=?1",
        [id],
        |r| {
            Ok(MachineFence {
                id: r.get(0)?,
                node_id: r.get(1)?,
                kind: r.get(2)?,
                status: r.get(3)?,
                owning_domain_id: r.get(4)?,
                capability_revision: r.get(5)?,
                security_epoch: r.get(6)?,
            })
        },
    ))
}

fn workspace_fence(c: &Connection, workspace_id: &str) -> Result<WorkspaceFence> {
    db(c.query_row(
        "SELECT domain_id,kind,host_machine_id,policy_revision,revision,archived_at
             FROM workspaces WHERE workspace_id=?1",
        [workspace_id],
        |r| {
            Ok(WorkspaceFence {
                domain_id: r.get(0)?,
                kind: r.get(1)?,
                host_machine_id: r.get(2)?,
                policy_revision: r.get(3)?,
                revision: r.get(4)?,
                archived: r.get::<_, Option<i64>>(5)?.is_some(),
            })
        },
    )
    .optional())?
    .ok_or(Error::Denied("remote-view Workspace is unavailable"))
}

fn validate_authenticated_device(
    c: &Connection,
    authenticated: &AuthenticatedStableUser,
) -> Result<DeviceFence> {
    if authenticated.method != "device_key"
        || authenticated.stable_principal != stable_user_principal(&authenticated.user_id)?.as_str()
    {
        return Err(Error::Denied(
            "remote view requires an exact stable device-key identity",
        ));
    }

    let user: Option<(String, u64)> = db(c
        .query_row(
            "SELECT status,security_epoch FROM users WHERE user_id=?1",
            [&authenticated.user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    if !matches!(user, Some((ref status, epoch)) if status == "active" && epoch == authenticated.user_security_epoch)
    {
        return Err(Error::Denied(
            "remote-view stable user identity is stale or inactive",
        ));
    }

    let domain: Option<(String, u64, u64)> = db(c
        .query_row(
            "SELECT status,policy_revision,security_epoch FROM domains WHERE domain_id=?1",
            [&authenticated.domain_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional())?;
    let Some((domain_status, domain_policy_revision, domain_security_epoch)) = domain else {
        return Err(Error::Denied(
            "remote-view Domain identity is stale or inactive",
        ));
    };
    if domain_status != "active" || domain_security_epoch != authenticated.domain_security_epoch {
        return Err(Error::Denied(
            "remote-view Domain identity is stale or inactive",
        ));
    }

    let binding: Option<(
        Option<String>,
        Option<String>,
        String,
        u64,
        u64,
        String,
        String,
    )> = db(c
        .query_row(
            "SELECT b.device_id,b.public_key,b.source_node_id,b.user_security_epoch,
                    b.domain_security_epoch,b.status,g.status
             FROM portable_identity_bindings b
             JOIN identity_continuity_grants g
               ON g.domain_id=b.domain_id AND g.source_node_id=b.source_node_id
             WHERE b.binding_id=?1 AND b.domain_id=?2 AND b.user_id=?3
               AND b.kind='device_key'",
            params![
                authenticated.binding_id,
                authenticated.domain_id,
                authenticated.user_id
            ],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional())?;
    let Some((
        Some(device_id),
        Some(public_key),
        source_node_id,
        user_epoch,
        domain_epoch,
        binding_state,
        grant_state,
    )) = binding
    else {
        return Err(Error::Denied(
            "remote-view device binding is unavailable or incomplete",
        ));
    };
    if binding_state != "active"
        || grant_state != "active"
        || user_epoch != authenticated.user_security_epoch
        || domain_epoch != authenticated.domain_security_epoch
    {
        return Err(Error::Denied(
            "remote-view device binding is stale or revoked",
        ));
    }
    let local_member: bool = db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM domain_memberships
            WHERE domain_id=?1 AND user_id=?2 AND status='active'
         )",
        params![authenticated.domain_id, authenticated.user_id],
        |r| r.get(0),
    ))?;
    // Workspace replicas intentionally do not import local Domain roles. For that
    // replica-only case, the latest active Phase-30 binding from the exact
    // recorded control AwNode is the conservative read-only membership
    // projection: source export includes only active Domain members, resync
    // revokes bindings absent from the next signed snapshot, and no owner/admin
    // role is materialized locally.
    let replica_member: bool = if local_member {
        false
    } else {
        db(c.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM workspace_replica_domains
                WHERE domain_id=?1 AND source_node_id=?2
             )",
            params![authenticated.domain_id, source_node_id],
            |r| r.get(0),
        ))?
    };
    if !local_member && !replica_member {
        return Err(Error::Denied(
            "remote-view user is no longer an active Domain member",
        ));
    }

    Ok(DeviceFence {
        binding_id: authenticated.binding_id.clone(),
        device_id,
        public_key,
        domain_policy_revision,
    })
}

fn validate_permissions(permissions: &BTreeSet<RemoteViewPermission>) -> Result<()> {
    if permissions.is_empty() || permissions.len() > 2 {
        return Err(Error::Denied("remote-view permission set is invalid"));
    }
    Ok(())
}

impl SessionStore {
    /// Create a reconnectable read-only remote-view grant. The caller must pass
    /// peer_node_id from the same authenticated MachineLink on which the device
    /// proof was established. Pairing alone is insufficient: the peer must also
    /// be an explicitly registered active remote Machine in the authenticated
    /// user's Domain.
    pub fn issue_remote_view_grant(
        &self,
        authenticated: &AuthenticatedStableUser,
        peer_node_id: &str,
        workspace_id: &str,
        permissions: BTreeSet<RemoteViewPermission>,
        ttl_secs: u64,
    ) -> Result<RemoteViewGrant> {
        validate_permissions(&permissions)?;
        if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&ttl_secs) {
            return Err(Error::Denied(
                "remote-view grant TTL must be between 5 seconds and 1 hour",
            ));
        }

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let device = validate_authenticated_device(&tx, authenticated)?;
        let peer = peer_fence(&tx, peer_node_id)?;
        let requester = machine_by_node(&tx, peer_node_id)?;
        if requester.kind != "remote"
            || requester.status != "active"
            || requester.owning_domain_id.as_deref() != Some(authenticated.domain_id.as_str())
        {
            return Err(Error::Denied(
                "remote-view requester is not an active registered Machine in this Domain",
            ));
        }

        let target = local_machine(&tx)?;
        if target.kind != "local" || target.status != "active" {
            return Err(Error::Denied(
                "remote-view target Machine is not locally active",
            ));
        }

        let workspace = workspace_fence(&tx, workspace_id)?;
        if workspace.archived
            || workspace.kind != "hosted"
            || workspace.domain_id != authenticated.domain_id
            || workspace.host_machine_id.as_deref() != Some(target.id.as_str())
        {
            return Err(Error::Denied(
                "remote-view Workspace is not an active hosted Workspace on this Machine",
            ));
        }

        let issued_at = now();
        let active: i64 = db(tx.query_row(
            "SELECT count(*) FROM remote_view_grants
             WHERE user_id=?1 AND status='active' AND expires_at>?2",
            params![authenticated.user_id, issued_at],
            |r| r.get(0),
        ))?;
        if active >= MAX_ACTIVE_GRANTS {
            return Err(Error::Limit("remote-view active grant limit reached"));
        }

        let ttl_micros = i64::try_from(ttl_secs)
            .ok()
            .and_then(|ttl| ttl.checked_mul(1_000_000))
            .ok_or(Error::Limit("remote-view grant expiry overflow"))?;
        let expires_at = issued_at
            .checked_add(ttl_micros)
            .ok_or(Error::Limit("remote-view grant expiry overflow"))?;
        let grant_id = id("rvg")?;
        db(tx.execute(
            "INSERT INTO remote_view_grants(
                grant_id,domain_id,workspace_id,workspace_revision,workspace_policy_revision,
                target_machine_id,target_machine_node_id,target_machine_security_epoch,target_machine_capability_revision,
                requester_machine_id,requester_machine_node_id,requester_machine_security_epoch,requester_machine_capability_revision,
                peer_id,peer_node_id,peer_security_epoch,peer_public_key,
                user_id,stable_principal,user_security_epoch,domain_policy_revision,domain_security_epoch,
                device_binding_id,device_id,device_public_key,permissions_json,status,revision,
                issued_at,expires_at,updated_at
             ) VALUES(
                ?1,?2,?3,?4,?5,
                ?6,?7,?8,?9,
                ?10,?11,?12,?13,
                ?14,?15,?16,?17,
                ?18,?19,?20,?21,?22,
                ?23,?24,?25,?26,'active',1,
                ?27,?28,?27
             )",
            params![
                grant_id,
                authenticated.domain_id,
                workspace_id,
                workspace.revision,
                workspace.policy_revision,
                target.id,
                target.node_id,
                target.security_epoch,
                target.capability_revision,
                requester.id,
                requester.node_id,
                requester.security_epoch,
                requester.capability_revision,
                peer.id,
                peer.node_id,
                peer.security_epoch,
                peer.public_key,
                authenticated.user_id,
                authenticated.stable_principal,
                authenticated.user_security_epoch,
                device.domain_policy_revision,
                authenticated.domain_security_epoch,
                device.binding_id,
                device.device_id,
                device.public_key,
                json(&permissions)?,
                issued_at,
                expires_at
            ],
        ))?;
        db(tx.commit())?;
        grant_row(&c, &grant_id)
    }

    /// Revalidate every durable authority fact for one remote-view read. A new
    /// tunnel/connection may reuse an unexpired grant, but a different peer,
    /// revoked device/source trust, changed user/Domain policy or security epoch,
    /// Machine/Workspace revision, or membership loss cannot.
    pub fn authorize_remote_view(
        &self,
        grant_id: &str,
        authenticated: &AuthenticatedStableUser,
        signed_peer_node_id: &str,
        permission: RemoteViewPermission,
    ) -> Result<RemoteViewGrant> {
        let c = self.connection()?;
        let grant = grant_row(&c, grant_id)?;
        if grant.state != RemoteViewGrantState::Active || grant.expires_at <= now() {
            return Err(Error::Denied("remote-view grant is revoked or expired"));
        }
        if !grant.permissions.contains(&permission) {
            return Err(Error::Denied(
                "remote-view grant does not permit this read surface",
            ));
        }

        let device = validate_authenticated_device(&c, authenticated)?;
        if grant.user_id != authenticated.user_id
            || grant.stable_principal != authenticated.stable_principal
            || grant.user_security_epoch != authenticated.user_security_epoch
            || grant.domain_id != authenticated.domain_id
            || grant.domain_policy_revision != device.domain_policy_revision
            || grant.domain_security_epoch != authenticated.domain_security_epoch
            || grant.device_binding_id != device.binding_id
            || grant.device_id != device.device_id
            || grant.device_public_key != device.public_key
        {
            return Err(Error::Denied(
                "remote-view authenticated device/user no longer matches the grant",
            ));
        }

        let peer = peer_fence(&c, signed_peer_node_id)?;
        if grant.peer_id != peer.id
            || grant.peer_node_id != peer.node_id
            || grant.peer_security_epoch != peer.security_epoch
            || grant.peer_public_key != peer.public_key
        {
            return Err(Error::Denied(
                "remote-view peer identity changed since grant issue",
            ));
        }

        let requester = machine_by_node(&c, signed_peer_node_id)?;
        if requester.kind != "remote"
            || requester.status != "active"
            || requester.owning_domain_id.as_deref() != Some(grant.domain_id.as_str())
            || grant.requester_machine_id != requester.id
            || grant.requester_machine_node_id != requester.node_id
            || grant.requester_machine_security_epoch != requester.security_epoch
            || grant.requester_machine_capability_revision != requester.capability_revision
        {
            return Err(Error::Denied(
                "remote-view requester Machine changed since grant issue",
            ));
        }

        let target = local_machine(&c)?;
        if target.kind != "local"
            || target.status != "active"
            || grant.target_machine_id != target.id
            || grant.target_machine_node_id != target.node_id
            || grant.target_machine_security_epoch != target.security_epoch
            || grant.target_machine_capability_revision != target.capability_revision
        {
            return Err(Error::Denied(
                "remote-view target Machine changed since grant issue",
            ));
        }

        let workspace = workspace_fence(&c, &grant.workspace_id)?;
        if workspace.archived
            || workspace.kind != "hosted"
            || workspace.domain_id != grant.domain_id
            || workspace.host_machine_id.as_deref() != Some(target.id.as_str())
            || workspace.revision != grant.workspace_revision
            || workspace.policy_revision != grant.workspace_policy_revision
        {
            return Err(Error::Denied(
                "remote-view Workspace changed since grant issue",
            ));
        }

        Ok(grant)
    }

    /// Explicit persisted revocation. The grantee may revoke its own view; a
    /// current Domain owner/admin may revoke any grant in that Domain. Trust,
    /// device, membership and epoch revocations also fail authorization without
    /// requiring this row to be rewritten.
    pub fn revoke_remote_view_grant(
        &self,
        actor: &PrincipalId,
        grant_id: &str,
    ) -> Result<RemoteViewGrant> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let grant = grant_row(&tx, grant_id)?;
        let actor_user = user_for_principal(&tx, actor)?.ok_or(Error::Denied(
            "remote-view grant revocation requires an active known user",
        ))?;
        let admin: bool = db(tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM domain_memberships
                WHERE domain_id=?1 AND user_id=?2 AND status='active'
                  AND role IN ('owner','admin')
             )",
            params![grant.domain_id, actor_user],
            |r| r.get(0),
        ))?;
        if actor_user != grant.user_id && !admin {
            return Err(Error::Denied(
                "remote-view grant revocation requires grantee or Domain owner/admin",
            ));
        }
        if grant.state != RemoteViewGrantState::Active {
            return Err(Error::Conflict("remote-view grant already revoked"));
        }
        let at = now();
        db(tx.execute(
            "UPDATE remote_view_grants
             SET status='revoked',revision=revision+1,updated_at=?1,revoked_at=?1
             WHERE grant_id=?2 AND status='active'",
            params![at, grant_id],
        ))?;
        db(tx.commit())?;
        grant_row(&c, grant_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionBinding;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    struct Fixture {
        store: SessionStore,
        owner: PrincipalId,
        authenticated: AuthenticatedStableUser,
        peer_node: String,
        workspace_id: String,
    }

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/fixture"),
            tool_root: PathBuf::from("/fixture"),
            system_prompt: String::new(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn node_for(byte: u8) -> (String, String) {
        let raw = [byte; 32];
        (
            format!("node_{:x}", Sha256::digest(raw)),
            raw.iter().map(|value| format!("{value:02x}")).collect(),
        )
    }

    fn fixture() -> Fixture {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("internal:remote-view:owner").unwrap();
        store
            .create(&owner, "remote-view fixture", binding())
            .unwrap();

        let domain = store.default_domain(&owner).unwrap();
        let workspace = store.local_workspace(&owner, Some(&domain.id)).unwrap();
        let (peer_node, peer_key) = node_for(0x11);
        store
            .trust_awnet_peer(&peer_node, &peer_key, "requester")
            .unwrap();
        store
            .register_remote_machine(&owner, Some(&domain.id), &peer_node, Some("requester"))
            .unwrap();

        let device_key = "22".repeat(32);
        let device = store
            .register_identity_device_key(&owner, &device_key)
            .unwrap();
        let user = store.current_user(&owner).unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let binding_id = "p_remote_view_fixture".to_owned();
        {
            let c = store.connection().unwrap();
            c.execute(
                "INSERT INTO identity_continuity_grants(
                    grant_id,domain_id,source_node_id,status,accepted_by_user_id,revision,accepted_at
                 ) VALUES('icg_remote_view',?1,?2,'active',?3,1,?4)",
                params![domain.id, peer_node, user.user_id, now()],
            )
            .unwrap();
            c.execute(
                "INSERT INTO portable_identity_bindings(
                    binding_id,domain_id,user_id,source_store_id,source_node_id,kind,binding_key,
                    device_id,public_key,user_security_epoch,domain_security_epoch,verified_at,
                    binding_sha256,status,imported_at
                 ) VALUES(?1,?2,?3,'source_store',?4,'device_key','device:remote-view',
                    ?5,?6,?7,?8,?9,?10,'active',?9)",
                params![
                    binding_id,
                    domain.id,
                    user.user_id,
                    peer_node,
                    device.device_id,
                    device.public_key,
                    user.security_epoch,
                    domain.security_epoch,
                    now(),
                    "a".repeat(64)
                ],
            )
            .unwrap();
        }
        let authenticated = AuthenticatedStableUser {
            user_id: user.user_id.clone(),
            stable_principal: stable_user_principal(&user.user_id)
                .unwrap()
                .as_str()
                .to_owned(),
            domain_id: domain.id,
            domain_role: Some("owner".into()),
            user_security_epoch: user.security_epoch,
            domain_security_epoch: domain.security_epoch,
            method: "device_key".into(),
            binding_id,
        };
        Fixture {
            store,
            owner,
            authenticated,
            peer_node,
            workspace_id: workspace.id,
        }
    }

    fn issue(f: &Fixture) -> RemoteViewGrant {
        f.store
            .issue_remote_view_grant(
                &f.authenticated,
                &f.peer_node,
                &f.workspace_id,
                BTreeSet::from([RemoteViewPermission::Files, RemoteViewPermission::Git]),
                60,
            )
            .unwrap()
    }

    #[test]
    fn remote_view_v34_to_v35_migration_preserves_store_identity() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::initialize(&mut c).unwrap();
        let store_id: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        c.execute_batch(
            "DROP TABLE remote_view_grants;
             UPDATE store_meta SET value='34' WHERE key='schema_epoch';
             UPDATE schema_meta
                SET schema_epoch=34,activation_epoch=34,migration_phase='ready',updated_at=0
              WHERE singleton=1;
             UPDATE migration_lease
                SET holder_client_id=NULL,generation=generation+1,current_epoch=34,
                    target_epoch=NULL,lease_expires_at=NULL,updated_at=0
              WHERE singleton=1;
             PRAGMA user_version=34;",
        )
        .unwrap();

        crate::initialize(&mut c).unwrap();
        let version: i64 = c
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let migrated_store: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let table_exists: bool = c
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master
                    WHERE type='table' AND name='remote_view_grants'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let check: String = c
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 35);
        assert_eq!(migrated_store, store_id);
        assert!(table_exists);
        assert_eq!(check, "ok");
    }

    #[test]
    fn remote_view_grant_is_reconnectable_only_for_exact_read_identity() {
        let f = fixture();
        let c = f.store.connection().unwrap();
        let version: i64 = c
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 35);
        drop(c);

        let grant = issue(&f);
        assert_eq!(grant.state, RemoteViewGrantState::Active);
        assert_eq!(
            f.store
                .authorize_remote_view(
                    &grant.grant_id,
                    &f.authenticated,
                    &f.peer_node,
                    RemoteViewPermission::Files,
                )
                .unwrap()
                .grant_id,
            grant.grant_id
        );
        assert!(f
            .store
            .authorize_remote_view(
                &grant.grant_id,
                &f.authenticated,
                &f.peer_node,
                RemoteViewPermission::Git,
            )
            .is_ok());

        let (other_node, other_key) = node_for(0x33);
        f.store
            .trust_awnet_peer(&other_node, &other_key, "unregistered")
            .unwrap();
        assert!(f
            .store
            .authorize_remote_view(
                &grant.grant_id,
                &f.authenticated,
                &other_node,
                RemoteViewPermission::Files,
            )
            .is_err());
        assert!(f
            .store
            .issue_remote_view_grant(
                &f.authenticated,
                &other_node,
                &f.workspace_id,
                BTreeSet::from([RemoteViewPermission::Files]),
                60,
            )
            .is_err());
    }

    #[test]
    fn remote_view_grant_revocation_scrubs_future_authorization() {
        let f = fixture();
        let grant = issue(&f);
        assert!(f
            .store
            .authorize_remote_view(
                &grant.grant_id,
                &f.authenticated,
                &f.peer_node,
                RemoteViewPermission::Files,
            )
            .is_ok());
        let revoked = f
            .store
            .revoke_remote_view_grant(&f.owner, &grant.grant_id)
            .unwrap();
        assert_eq!(revoked.state, RemoteViewGrantState::Revoked);
        assert_eq!(revoked.revision, grant.revision + 1);
        assert!(f
            .store
            .authorize_remote_view(
                &grant.grant_id,
                &f.authenticated,
                &f.peer_node,
                RemoteViewPermission::Files,
            )
            .is_err());
    }

    #[test]
    fn remote_view_grant_fails_closed_when_any_pinned_authority_changes() {
        let cases: &[(&str, &str)] = &[
            (
                "UPDATE awnet_peers SET security_epoch=security_epoch+1 WHERE node_id=?1",
                "peer",
            ),
            (
                "UPDATE machines SET capability_revision=capability_revision+1 WHERE node_id=?1",
                "requester",
            ),
            (
                "UPDATE machines SET security_epoch=security_epoch+1 WHERE kind='local'",
                "target",
            ),
            (
                "UPDATE workspaces SET revision=revision+1 WHERE workspace_id=?1",
                "workspace",
            ),
            (
                "UPDATE users SET security_epoch=security_epoch+1 WHERE user_id=?1",
                "user",
            ),
            (
                "UPDATE domains SET policy_revision=policy_revision+1 WHERE domain_id=?1",
                "domain_policy",
            ),
            (
                "UPDATE domain_memberships SET status='revoked' WHERE domain_id=?1 AND user_id=?2",
                "membership",
            ),
            (
                "UPDATE portable_identity_bindings SET status='revoked' WHERE binding_id=?1",
                "device",
            ),
            (
                "UPDATE identity_continuity_grants SET status='revoked' WHERE domain_id=?1 AND source_node_id=?2",
                "source",
            ),
        ];
        for (sql, kind) in cases {
            let f = fixture();
            let grant = issue(&f);
            {
                let c = f.store.connection().unwrap();
                match *kind {
                    "peer" | "requester" => {
                        c.execute(sql, [&f.peer_node]).unwrap();
                    }
                    "target" => {
                        c.execute(sql, []).unwrap();
                    }
                    "workspace" => {
                        c.execute(sql, [&f.workspace_id]).unwrap();
                    }
                    "user" => {
                        c.execute(sql, [&f.authenticated.user_id]).unwrap();
                    }
                    "domain_policy" => {
                        c.execute(sql, [&f.authenticated.domain_id]).unwrap();
                    }
                    "membership" => {
                        c.execute(
                            sql,
                            params![f.authenticated.domain_id, f.authenticated.user_id],
                        )
                        .unwrap();
                    }
                    "device" => {
                        c.execute(sql, [&f.authenticated.binding_id]).unwrap();
                    }
                    "source" => {
                        c.execute(sql, params![f.authenticated.domain_id, f.peer_node])
                            .unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            assert!(
                f.store
                    .authorize_remote_view(
                        &grant.grant_id,
                        &f.authenticated,
                        &f.peer_node,
                        RemoteViewPermission::Files,
                    )
                    .is_err(),
                "{kind} change must fence the grant"
            );
        }

        let f = fixture();
        let grant = issue(&f);
        {
            let c = f.store.connection().unwrap();
            c.execute(
                "UPDATE remote_view_grants SET expires_at=?1 WHERE grant_id=?2",
                params![now().saturating_sub(1), grant.grant_id],
            )
            .unwrap();
        }
        assert!(f
            .store
            .authorize_remote_view(
                &grant.grant_id,
                &f.authenticated,
                &f.peer_node,
                RemoteViewPermission::Files,
            )
            .is_err());
    }
}
