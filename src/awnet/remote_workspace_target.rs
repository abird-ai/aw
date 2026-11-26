use super::machinelink::{MachineCapability, MachineLinkConnectOptions};
use super::reachability::{validate_room, PunchOptions};
use super::{
    ensure_identity, MachineReadRequest, MachineServiceRequestOptions, RemoteViewReadOptions,
    RemoteWorkspaceRequest,
};
use aw::{Error, PrincipalId, Result};
use aw_store::{RemoteViewGrant, RemoteViewGrantState, RemoteViewPermission, SessionStore};
use ed25519_dalek::SigningKey;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fmt,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DEFAULT_LEASE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteWorkspaceTargetDescriptor {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub files: bool,
    pub git: bool,
    pub expires_at: i64,
}

#[derive(Clone)]
pub struct RemoteWorkspaceTargetAdmission {
    descriptor: RemoteWorkspaceTargetDescriptor,
    route: PunchOptions,
    grant: RemoteViewGrant,
    signing_key: SigningKey,
    lease: Duration,
}

impl fmt::Debug for RemoteWorkspaceTargetAdmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteWorkspaceTargetAdmission")
            .field("descriptor", &self.descriptor)
            .field("route_peer", &self.route.peer_node_id)
            .field("permissions", &self.grant.permissions)
            .field("lease", &self.lease)
            .finish_non_exhaustive()
    }
}

fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}

fn target_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Storage("OS randomness unavailable for remote Workspace target"))?;
    Ok(format!(
        "rwt_{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn bounded_label(value: &str) -> String {
    let clean = value
        .chars()
        .filter(|character| !character.is_control())
        .take(72)
        .collect::<String>();
    if clean.trim().is_empty() {
        "Remote machine".into()
    } else {
        clean
    }
}

impl RemoteWorkspaceTargetAdmission {
    fn validate(
        store: &SessionStore,
        actor: &PrincipalId,
        state_dir: &Path,
        route: &PunchOptions,
        grant: &RemoteViewGrant,
        signing_key: &SigningKey,
    ) -> Result<String> {
        validate_room(&route.room)?;
        if grant.state != RemoteViewGrantState::Active || grant.expires_at <= now_micros() {
            return Err(Error::Denied(
                "remote Workspace grant is revoked or expired",
            ));
        }
        if grant.permissions.is_empty() {
            return Err(Error::Denied(
                "remote Workspace grant has no read permission",
            ));
        }
        if route.peer_node_id != grant.target_machine_node_id {
            return Err(Error::Denied(
                "remote Workspace route does not target the granted Machine",
            ));
        }

        let identity = ensure_identity(store, state_dir)?;
        let local_machine = store.local_machine()?;
        if identity.node_id != local_machine.node_id
            || grant.requester_machine_node_id != identity.node_id
        {
            return Err(Error::Denied(
                "remote Workspace grant requester is not this current AwNode",
            ));
        }

        let peer = store.awnet_peer_by_node_id(&route.peer_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied("remote Workspace target peer is not trusted"));
        }
        let machine = store.machine(actor, Some(&grant.domain_id), &route.peer_node_id)?;
        if machine.node_id != route.peer_node_id
            || machine.kind != "remote"
            || machine.status != "active"
            || machine.owning_domain_id.as_deref() != Some(grant.domain_id.as_str())
        {
            return Err(Error::Denied(
                "remote Workspace target is not an active registered Machine in the granted Domain",
            ));
        }

        let domain = store.domain(actor, &grant.domain_id)?;
        if domain.status != "active"
            || domain.policy_revision != grant.domain_policy_revision
            || domain.security_epoch != grant.domain_security_epoch
        {
            return Err(Error::Denied(
                "remote Workspace Domain policy/security changed since grant issue",
            ));
        }
        let user = store.known_user(actor)?;
        if user.user_id != grant.user_id || user.security_epoch != grant.user_security_epoch {
            return Err(Error::Denied(
                "remote Workspace stable user changed since grant issue",
            ));
        }

        let public_key = super::hex(&signing_key.verifying_key().to_bytes());
        if public_key != grant.device_public_key {
            return Err(Error::Denied(
                "remote Workspace device key does not match the grant",
            ));
        }
        let device = store.identity_device_key(actor, &public_key)?;
        if device.user_id != grant.user_id || device.status != "active" {
            return Err(Error::Denied(
                "remote Workspace device is no longer active for this user",
            ));
        }
        Ok(bounded_label(&machine.display_name))
    }

    pub fn admit(
        store: &SessionStore,
        actor: &PrincipalId,
        state_dir: &Path,
        route: PunchOptions,
        grant: RemoteViewGrant,
        signing_key: SigningKey,
    ) -> Result<Self> {
        let label = Self::validate(store, actor, state_dir, &route, &grant, &signing_key)?;
        Ok(Self {
            descriptor: RemoteWorkspaceTargetDescriptor {
                id: target_id()?,
                kind: "machine".into(),
                label,
                files: grant.permissions.contains(&RemoteViewPermission::Files),
                git: grant.permissions.contains(&RemoteViewPermission::Git),
                expires_at: grant.expires_at,
            },
            route,
            grant,
            signing_key,
            lease: DEFAULT_LEASE,
        })
    }

    pub fn descriptor(&self) -> &RemoteWorkspaceTargetDescriptor {
        &self.descriptor
    }

    pub fn revalidate(
        &self,
        store: &SessionStore,
        actor: &PrincipalId,
        state_dir: &Path,
    ) -> Result<()> {
        Self::validate(
            store,
            actor,
            state_dir,
            &self.route,
            &self.grant,
            &self.signing_key,
        )
        .map(|_| ())
    }

    pub fn request_options(
        &self,
        workspace: RemoteWorkspaceRequest,
    ) -> Result<MachineServiceRequestOptions> {
        if self.grant.state != RemoteViewGrantState::Active || self.grant.expires_at <= now_micros()
        {
            return Err(Error::Denied(
                "remote Workspace grant is revoked or expired",
            ));
        }
        let permission = workspace.permission();
        if !self.grant.permissions.contains(&permission) {
            return Err(Error::Denied(
                "remote Workspace target does not grant this read surface",
            ));
        }
        let required = match permission {
            RemoteViewPermission::Files => MachineCapability::FileRead,
            RemoteViewPermission::Git => MachineCapability::GitRead,
        };
        Ok(MachineServiceRequestOptions {
            link: MachineLinkConnectOptions {
                route: self.route.clone(),
                requested: BTreeSet::from([MachineCapability::IdentityAuthenticate, required]),
                lease: self.lease,
            },
            request: MachineReadRequest::RemoteWorkspace { workspace },
            remote_view: Some(RemoteViewReadOptions {
                grant: self.grant.clone(),
                signing_key: self.signing_key.clone(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{fs, path::PathBuf};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let mut entropy = [0u8; 8];
            getrandom::fill(&mut entropy).unwrap();
            let suffix = entropy
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let path = std::env::temp_dir().join(format!("aw-{name}-{suffix}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn node_for(byte: u8) -> (String, String) {
        let raw = [byte; 32];
        (
            format!("node_{:x}", Sha256::digest(raw)),
            raw.iter().map(|value| format!("{value:02x}")).collect(),
        )
    }

    struct Fixture {
        _dir: TempDir,
        state_dir: TempDir,
        store: SessionStore,
        actor: PrincipalId,
        route: PunchOptions,
        grant: RemoteViewGrant,
        signing_key: SigningKey,
        device_id: String,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new("remote-target-store");
        let state_dir = TempDir::new("remote-target-state");
        let store = SessionStore::open(dir.path()).unwrap();
        let actor = PrincipalId::new("internal:remote-target:owner").unwrap();
        let _ = store.current_user(&actor).unwrap();
        let local = ensure_identity(&store, state_dir.path()).unwrap();
        let domain = store.default_domain(&actor).unwrap();

        let (target_node, target_key) = node_for(0x44);
        store
            .trust_awnet_peer(&target_node, &target_key, "target-machine")
            .unwrap();
        store
            .register_remote_machine(
                &actor,
                Some(&domain.id),
                &target_node,
                Some("target-machine"),
            )
            .unwrap();

        let signing_key = SigningKey::from_bytes(&[0x22; 32]);
        let public_key = super::super::hex(&signing_key.verifying_key().to_bytes());
        let device = store
            .register_identity_device_key(&actor, &public_key)
            .unwrap();
        let user = store.current_user(&actor).unwrap();
        let domain = store.domain(&actor, &domain.id).unwrap();
        let now = now_micros();
        let grant = RemoteViewGrant {
            grant_id: "rvg_fixture".into(),
            domain_id: domain.id.clone(),
            workspace_id: "ws_remote_fixture".into(),
            workspace_revision: 7,
            workspace_policy_revision: 3,
            target_machine_id: "remote-server-machine-id".into(),
            target_machine_node_id: target_node.clone(),
            target_machine_security_epoch: 2,
            target_machine_capability_revision: 4,
            requester_machine_id: "remote-server-record-of-requester".into(),
            requester_machine_node_id: local.node_id.clone(),
            requester_machine_security_epoch: 2,
            requester_machine_capability_revision: 3,
            peer_id: "remote-server-peer-record".into(),
            peer_node_id: local.node_id,
            peer_security_epoch: 1,
            peer_public_key: "11".repeat(32),
            user_id: user.user_id,
            stable_principal: format!("user:{}", "fixture"),
            user_security_epoch: user.security_epoch,
            domain_policy_revision: domain.policy_revision,
            domain_security_epoch: domain.security_epoch,
            device_binding_id: "p_remote_fixture".into(),
            device_id: "udev_remote_fixture".into(),
            device_public_key: public_key,
            permissions: BTreeSet::from([RemoteViewPermission::Files, RemoteViewPermission::Git]),
            state: RemoteViewGrantState::Active,
            revision: 1,
            issued_at: now,
            expires_at: now + 60_000_000,
            updated_at: now,
            revoked_at: None,
        };
        Fixture {
            _dir: dir,
            state_dir,
            store,
            actor,
            route: PunchOptions {
                peer_node_id: target_node,
                room: "ab".repeat(32),
                rendezvous: "127.0.0.1:40000".parse().unwrap(),
                listen: "127.0.0.1:0".parse().unwrap(),
                stun: vec![],
                timeout: Duration::from_secs(5),
            },
            grant,
            signing_key,
            device_id: device.device_id,
        }
    }

    #[test]
    fn admitted_target_exposes_only_opaque_descriptor_and_builds_exact_read_options() {
        let fixture = fixture();
        let admission = RemoteWorkspaceTargetAdmission::admit(
            &fixture.store,
            &fixture.actor,
            fixture.state_dir.path(),
            fixture.route.clone(),
            fixture.grant.clone(),
            fixture.signing_key.clone(),
        )
        .unwrap();
        let descriptor = serde_json::to_value(admission.descriptor()).unwrap();
        assert_eq!(descriptor["kind"], "machine");
        assert_eq!(descriptor["files"], true);
        assert_eq!(descriptor["git"], true);
        let encoded = descriptor.to_string();
        assert!(!encoded.contains(&fixture.route.peer_node_id));
        assert!(!encoded.contains(&fixture.route.room));
        assert!(!encoded.contains(&fixture.grant.grant_id));
        assert!(!encoded.contains(&fixture.grant.workspace_id));
        assert!(!encoded.contains(&fixture.grant.device_public_key));

        let options = admission
            .request_options(RemoteWorkspaceRequest::Capabilities {
                permission: RemoteViewPermission::Git,
            })
            .unwrap();
        assert!(options
            .link
            .requested
            .contains(&MachineCapability::IdentityAuthenticate));
        assert!(options.link.requested.contains(&MachineCapability::GitRead));
        assert!(!options
            .link
            .requested
            .contains(&MachineCapability::FileRead));
    }

    #[test]
    fn admitted_target_is_fenced_after_local_device_revocation() {
        let fixture = fixture();
        let admission = RemoteWorkspaceTargetAdmission::admit(
            &fixture.store,
            &fixture.actor,
            fixture.state_dir.path(),
            fixture.route.clone(),
            fixture.grant.clone(),
            fixture.signing_key.clone(),
        )
        .unwrap();
        assert!(admission
            .revalidate(&fixture.store, &fixture.actor, fixture.state_dir.path())
            .is_ok());
        fixture
            .store
            .revoke_identity_device_key(&fixture.actor, &fixture.device_id)
            .unwrap();
        assert!(admission
            .revalidate(&fixture.store, &fixture.actor, fixture.state_dir.path())
            .is_err());
    }

    #[test]
    fn admission_fails_closed_on_route_key_user_or_policy_drift() {
        let fixture = fixture();

        let mut route = fixture.route.clone();
        route.peer_node_id = node_for(0x55).0;
        assert!(RemoteWorkspaceTargetAdmission::admit(
            &fixture.store,
            &fixture.actor,
            fixture.state_dir.path(),
            route,
            fixture.grant.clone(),
            fixture.signing_key.clone(),
        )
        .is_err());

        assert!(RemoteWorkspaceTargetAdmission::admit(
            &fixture.store,
            &fixture.actor,
            fixture.state_dir.path(),
            fixture.route.clone(),
            fixture.grant.clone(),
            SigningKey::from_bytes(&[0x33; 32]),
        )
        .is_err());

        let mut stale = fixture.grant.clone();
        stale.user_security_epoch += 1;
        assert!(RemoteWorkspaceTargetAdmission::admit(
            &fixture.store,
            &fixture.actor,
            fixture.state_dir.path(),
            fixture.route.clone(),
            stale,
            fixture.signing_key.clone(),
        )
        .is_err());

        let mut stale = fixture.grant.clone();
        stale.domain_policy_revision += 1;
        assert!(RemoteWorkspaceTargetAdmission::admit(
            &fixture.store,
            &fixture.actor,
            fixture.state_dir.path(),
            fixture.route,
            stale,
            fixture.signing_key,
        )
        .is_err());
    }
}
