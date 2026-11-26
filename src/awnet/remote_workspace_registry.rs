use super::remote_workspace_target::RemoteWorkspaceTargetAdmission;
use super::{
    request_identity_auth, request_machine_service, IdentityAuthRequestOptions, MachineCapability,
    MachineLinkConnectOptions, MachineReadResponse, PunchOptions, RemoteViewGrantRequest,
    RemoteWorkspaceErrorCode, RemoteWorkspaceGitMode, RemoteWorkspaceRequest,
    RemoteWorkspaceResponse, RemoteWorkspaceViewRequest,
};
use aw::web::{
    RemoteWorkspaceTargetDescriptor as WebTargetDescriptor, RemoteWorkspaceTargetError,
    RemoteWorkspaceTargetErrorCode as WebErrorCode, RemoteWorkspaceTargetRouter,
};
use aw::{Error, PrincipalId, Result};
use aw_store::{RemoteViewPermission, SessionStore};
use aw_surfaces::{Command, GitMode, ViewRequest};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

fn default_lease_secs() -> u64 {
    60
}
fn default_view_ttl_secs() -> u64 {
    600
}
fn default_timeout_secs() -> u64 {
    30
}
fn default_listen() -> SocketAddr {
    "0.0.0.0:0".parse().expect("literal socket")
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteWorkspaceTargetConfig {
    version: u16,
    peer_node_id: String,
    room: String,
    rendezvous: SocketAddr,
    domain_id: String,
    workspace_id: String,
    device_key: PathBuf,
    permissions: BTreeSet<RemoteViewPermission>,
    #[serde(default = "default_lease_secs")]
    lease_secs: u64,
    #[serde(default = "default_view_ttl_secs")]
    view_ttl_secs: u64,
    #[serde(default = "default_listen")]
    listen: SocketAddr,
    #[serde(default)]
    stun: Vec<SocketAddr>,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl RemoteWorkspaceTargetConfig {
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Config(
                "remote Workspace target config version must be 1".into(),
            ));
        }
        if self.permissions.is_empty() || self.permissions.len() > 2 {
            return Err(Error::Config(
                "remote Workspace target requires files and/or git permission".into(),
            ));
        }
        if !(5..=300).contains(&self.lease_secs) {
            return Err(Error::Config(
                "remote Workspace MachineLink lease must be 5..=300 seconds".into(),
            ));
        }
        if !(5..=3600).contains(&self.view_ttl_secs) {
            return Err(Error::Config(
                "remote Workspace view TTL must be 5..=3600 seconds".into(),
            ));
        }
        if !(1..=300).contains(&self.timeout_secs) {
            return Err(Error::Config(
                "remote Workspace network timeout must be 1..=300 seconds".into(),
            ));
        }
        if !self.device_key.is_absolute() {
            return Err(Error::Config(
                "remote Workspace device_key must be an absolute owner-private path".into(),
            ));
        }
        super::reachability::validate_room(&self.room)?;
        Ok(())
    }

    fn route(&self) -> PunchOptions {
        PunchOptions {
            peer_node_id: self.peer_node_id.clone(),
            room: self.room.clone(),
            rendezvous: self.rendezvous,
            listen: self.listen,
            stun: self.stun.clone(),
            timeout: Duration::from_secs(self.timeout_secs),
        }
    }
}

fn load_config(path: &Path) -> Result<RemoteWorkspaceTargetConfig> {
    let mut file = aw_platform::open_private_existing(path, false).map_err(|_| {
        Error::Denied("remote Workspace target config requires owner-private custody")
    })?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(Error::Limit(
            "remote Workspace target config exceeds 64 KiB",
        ));
    }
    let config: RemoteWorkspaceTargetConfig = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Storage("invalid remote Workspace target config"))?;
    config.validate()?;
    Ok(config)
}

fn web_descriptor(target: &RemoteWorkspaceTargetAdmission) -> WebTargetDescriptor {
    let descriptor = target.descriptor();
    WebTargetDescriptor {
        id: descriptor.id.clone(),
        kind: descriptor.kind.clone(),
        label: descriptor.label.clone(),
        files: descriptor.files,
        git: descriptor.git,
        expires_at: descriptor.expires_at,
    }
}

fn denied(
    message: &'static str,
    source_id: Option<String>,
    scope_id: Option<String>,
) -> RemoteWorkspaceTargetError {
    RemoteWorkspaceTargetError {
        code: WebErrorCode::Denied,
        message,
        source_id,
        scope_id,
    }
}

fn unavailable(message: &'static str) -> RemoteWorkspaceTargetError {
    RemoteWorkspaceTargetError {
        code: WebErrorCode::Unavailable,
        message,
        source_id: None,
        scope_id: None,
    }
}

fn request_failure(error: Error) -> RemoteWorkspaceTargetError {
    match error {
        Error::Denied(_) => RemoteWorkspaceTargetError {
            code: WebErrorCode::Denied,
            message: "remote Workspace grant or read authority was denied",
            source_id: None,
            scope_id: None,
        },
        Error::Conflict(_) => RemoteWorkspaceTargetError {
            code: WebErrorCode::Conflict,
            message: "remote Workspace authority or snapshot changed",
            source_id: None,
            scope_id: None,
        },
        Error::Limit(_) => RemoteWorkspaceTargetError {
            code: WebErrorCode::Limit,
            message: "remote Workspace read exceeded a bounded limit",
            source_id: None,
            scope_id: None,
        },
        Error::Config(_)
        | Error::Io(_)
        | Error::BeforeDispatch(_)
        | Error::Transport(_)
        | Error::Http { .. }
        | Error::Protocol(_)
        | Error::Cancelled
        | Error::Storage(_)
        | Error::StorageDetail(_)
        | Error::Timeout => unavailable("remote Workspace target unavailable"),
    }
}

fn remote_error(
    code: RemoteWorkspaceErrorCode,
    source_id: Option<String>,
    scope_id: Option<String>,
) -> RemoteWorkspaceTargetError {
    let (code, message) = match code {
        RemoteWorkspaceErrorCode::SourceChanged => (
            WebErrorCode::SourceChanged,
            "remote Workspace source changed; refresh target capabilities",
        ),
        RemoteWorkspaceErrorCode::ScopeChanged => (
            WebErrorCode::ScopeChanged,
            "remote Workspace scope changed; refresh the exact target",
        ),
        RemoteWorkspaceErrorCode::Denied => (WebErrorCode::Denied, "remote Workspace read denied"),
        RemoteWorkspaceErrorCode::Invalid => {
            (WebErrorCode::Invalid, "remote Workspace request invalid")
        }
        RemoteWorkspaceErrorCode::Conflict => (
            WebErrorCode::Conflict,
            "remote Workspace snapshot changed; refresh explicitly",
        ),
        RemoteWorkspaceErrorCode::Limit => (
            WebErrorCode::Limit,
            "remote Workspace bounded view limit reached",
        ),
        RemoteWorkspaceErrorCode::Unavailable => (
            WebErrorCode::Unavailable,
            "remote Workspace view unavailable",
        ),
    };
    RemoteWorkspaceTargetError {
        code,
        message,
        source_id,
        scope_id,
    }
}

fn map_view_request(request: ViewRequest) -> RemoteWorkspaceViewRequest {
    match request {
        ViewRequest::Files { path } => RemoteWorkspaceViewRequest::Files { path },
        ViewRequest::FileSearch { query, limit } => {
            RemoteWorkspaceViewRequest::FileSearch { query, limit }
        }
        ViewRequest::File { path } => RemoteWorkspaceViewRequest::File { path },
        ViewRequest::GitList { mode } => RemoteWorkspaceViewRequest::GitList {
            mode: match mode {
                GitMode::Changes => RemoteWorkspaceGitMode::Changes,
                GitMode::Worktrees => RemoteWorkspaceGitMode::Worktrees,
                GitMode::Branches => RemoteWorkspaceGitMode::Branches,
            },
        },
        ViewRequest::GitOpen {
            inventory_id,
            item_id,
        } => RemoteWorkspaceViewRequest::GitOpen {
            inventory_id,
            item_id,
        },
        ViewRequest::Review { review_id } => RemoteWorkspaceViewRequest::Review { review_id },
    }
}

pub struct RemoteWorkspaceTargetRegistry {
    store: SessionStore,
    actor: PrincipalId,
    state_dir: PathBuf,
    targets: BTreeMap<String, Arc<RemoteWorkspaceTargetAdmission>>,
}

impl fmt::Debug for RemoteWorkspaceTargetRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteWorkspaceTargetRegistry")
            .field(
                "targets",
                &self
                    .targets
                    .values()
                    .map(|target| target.descriptor())
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl RemoteWorkspaceTargetRegistry {
    pub fn connect_private_configs(
        store: SessionStore,
        actor: PrincipalId,
        state_dir: PathBuf,
        paths: &[PathBuf],
    ) -> Result<Arc<Self>> {
        if paths.is_empty() {
            return Ok(Arc::new(Self {
                store,
                actor,
                state_dir,
                targets: BTreeMap::new(),
            }));
        }
        let mut targets = BTreeMap::new();
        for path in paths {
            let config = load_config(path)?;
            let peer = store.awnet_peer_by_node_id(&config.peer_node_id)?;
            if peer.trust_state != "trusted" {
                return Err(Error::Denied(
                    "remote Workspace target peer is not currently trusted",
                ));
            }
            let domain = store.domain(&actor, &config.domain_id)?;
            if domain.id != config.domain_id || domain.status != "active" {
                return Err(Error::Denied(
                    "remote Workspace target Domain is unavailable or not exact",
                ));
            }
            let machine = store.machine(&actor, Some(&domain.id), &config.peer_node_id)?;
            if machine.node_id != config.peer_node_id
                || machine.kind != "remote"
                || machine.status != "active"
                || machine.owning_domain_id.as_deref() != Some(domain.id.as_str())
            {
                return Err(Error::Denied(
                    "remote Workspace target is not an active registered Machine in the exact Domain",
                ));
            }

            let signing_key = crate::resources_cli::load_user_device_key(&config.device_key)?;
            let public_key = super::hex(&signing_key.verifying_key().to_bytes());
            let device = store.identity_device_key(&actor, &public_key)?;
            if device.status != "active" {
                return Err(Error::Denied(
                    "remote Workspace target device key is no longer active",
                ));
            }

            let mut requested = BTreeSet::from([MachineCapability::IdentityAuthenticate]);
            if config.permissions.contains(&RemoteViewPermission::Files) {
                requested.insert(MachineCapability::FileRead);
            }
            if config.permissions.contains(&RemoteViewPermission::Git) {
                requested.insert(MachineCapability::GitRead);
            }
            let route = config.route();
            let result = request_identity_auth(
                &store,
                &state_dir,
                IdentityAuthRequestOptions {
                    link: MachineLinkConnectOptions {
                        route: route.clone(),
                        requested,
                        lease: Duration::from_secs(config.lease_secs),
                    },
                    domain_id: config.domain_id,
                    session_id: None,
                    auth_session_ttl: None,
                    remote_view: Some(RemoteViewGrantRequest {
                        workspace_id: config.workspace_id,
                        permissions: config.permissions,
                        ttl_secs: config.view_ttl_secs,
                    }),
                    signing_key: signing_key.clone(),
                },
            )?;
            let grant = result.remote_view_grant.ok_or(Error::Denied(
                "remote AwNode did not issue the requested Workspace view grant",
            ))?;
            let admission = Arc::new(RemoteWorkspaceTargetAdmission::admit(
                &store,
                &actor,
                &state_dir,
                route,
                grant,
                signing_key,
            )?);
            let id = admission.descriptor().id.clone();
            if targets.insert(id, admission).is_some() {
                return Err(Error::Conflict(
                    "remote Workspace opaque target ID collision",
                ));
            }
        }
        Ok(Arc::new(Self {
            store,
            actor,
            state_dir,
            targets,
        }))
    }

    fn target(
        &self,
        target_id: &str,
    ) -> std::result::Result<Arc<RemoteWorkspaceTargetAdmission>, RemoteWorkspaceTargetError> {
        let target = self
            .targets
            .get(target_id)
            .cloned()
            .ok_or_else(|| denied("remote Workspace target unavailable", None, None))?;
        target
            .revalidate(&self.store, &self.actor, &self.state_dir)
            .map_err(|_| denied("remote Workspace target authority changed", None, None))?;
        Ok(target)
    }

    fn request(
        &self,
        target: &RemoteWorkspaceTargetAdmission,
        request: RemoteWorkspaceRequest,
    ) -> std::result::Result<RemoteWorkspaceResponse, RemoteWorkspaceTargetError> {
        let options = target
            .request_options(request)
            .map_err(|_| denied("remote Workspace read permission changed", None, None))?;
        let result = request_machine_service(&self.store, &self.state_dir, options)
            .map_err(request_failure)?;
        match result.response {
            MachineReadResponse::RemoteWorkspace { workspace } => Ok(workspace),
            _ => Err(unavailable(
                "remote Workspace target returned an unexpected read response",
            )),
        }
    }

    fn permission_capabilities(
        &self,
        target: &RemoteWorkspaceTargetAdmission,
        permission: RemoteViewPermission,
    ) -> std::result::Result<Value, RemoteWorkspaceTargetError> {
        match self.request(target, RemoteWorkspaceRequest::Capabilities { permission })? {
            RemoteWorkspaceResponse::Capabilities { capabilities } => Ok(capabilities),
            RemoteWorkspaceResponse::Error {
                source_id,
                scope_id,
                code,
                ..
            } => Err(remote_error(code, source_id, scope_id)),
            _ => Err(unavailable(
                "remote Workspace target returned an unexpected capability response",
            )),
        }
    }

    fn merged_capabilities(
        &self,
        target: &RemoteWorkspaceTargetAdmission,
    ) -> std::result::Result<Value, RemoteWorkspaceTargetError> {
        let descriptor = target.descriptor();
        let files = descriptor
            .files
            .then(|| self.permission_capabilities(target, RemoteViewPermission::Files))
            .transpose()?;
        let git = descriptor
            .git
            .then(|| self.permission_capabilities(target, RemoteViewPermission::Git))
            .transpose()?;
        let mut merged = files
            .clone()
            .or_else(|| git.clone())
            .ok_or_else(|| denied("remote Workspace target has no read permission", None, None))?;
        if let (Some(files), Some(git)) = (&files, &git) {
            if files["source_id"] != git["source_id"] || files["view_root"] != git["view_root"] {
                return Err(RemoteWorkspaceTargetError {
                    code: WebErrorCode::Conflict,
                    message: "remote Workspace capability roots disagree",
                    source_id: None,
                    scope_id: None,
                });
            }
        }
        let object = merged
            .as_object_mut()
            .ok_or_else(|| unavailable("remote Workspace capability payload is malformed"))?;
        object.insert("files".into(), Value::Bool(descriptor.files));
        object.insert("git_read".into(), Value::Bool(descriptor.git));
        object.insert("git_write".into(), Value::Bool(false));
        object.insert("terminals".into(), Value::Bool(false));
        object.insert("sites".into(), Value::Array(Vec::new()));
        object.insert("max_terminals".into(), Value::from(0));
        object.insert("terminal_lease_seconds".into(), Value::from(0));
        Ok(merged)
    }
}

impl RemoteWorkspaceTargetRouter for RemoteWorkspaceTargetRegistry {
    fn descriptors(&self) -> Vec<WebTargetDescriptor> {
        self.targets
            .values()
            .filter(|target| {
                target
                    .revalidate(&self.store, &self.actor, &self.state_dir)
                    .is_ok()
            })
            .map(|target| web_descriptor(target))
            .collect()
    }

    fn capabilities(
        &self,
        target_id: &str,
    ) -> std::result::Result<Value, RemoteWorkspaceTargetError> {
        let target = self.target(target_id)?;
        self.merged_capabilities(&target)
    }

    fn execute(
        &self,
        target_id: &str,
        command: Command,
    ) -> std::result::Result<Value, RemoteWorkspaceTargetError> {
        let target = self.target(target_id)?;
        let Command::View {
            source_id,
            scope_id,
            request,
        } = command
        else {
            return Err(denied(
                "remote Workspace target accepts read-only View commands only",
                None,
                None,
            ));
        };
        let request = RemoteWorkspaceRequest::View {
            source_id: source_id.clone(),
            scope_id: scope_id.clone(),
            request: map_view_request(request),
        };
        match self.request(&target, request)? {
            RemoteWorkspaceResponse::View { envelope } => Ok(envelope),
            RemoteWorkspaceResponse::Error {
                source_id,
                scope_id,
                code,
                ..
            } => Err(remote_error(code, source_id, scope_id)),
            _ => Err(unavailable(
                "remote Workspace target returned an unexpected view response",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_empty_permissions_and_out_of_bounds_lifetimes() {
        let base = RemoteWorkspaceTargetConfig {
            version: 1,
            peer_node_id: "node_fixture".into(),
            room: "ab".repeat(32),
            rendezvous: "127.0.0.1:40000".parse().unwrap(),
            domain_id: "dom_fixture".into(),
            workspace_id: "ws_fixture".into(),
            device_key: PathBuf::from("/private/device-key.json"),
            permissions: BTreeSet::new(),
            lease_secs: 60,
            view_ttl_secs: 600,
            listen: "127.0.0.1:0".parse().unwrap(),
            stun: Vec::new(),
            timeout_secs: 30,
        };
        assert!(base.validate().is_err());
        let mut config = base.clone();
        config.permissions.insert(RemoteViewPermission::Files);
        assert!(config.validate().is_ok());
        config.lease_secs = 301;
        assert!(config.validate().is_err());
        config.lease_secs = 60;
        config.view_ttl_secs = 3601;
        assert!(config.validate().is_err());
    }

    #[test]
    fn view_mapping_is_read_only_and_exact() {
        assert!(matches!(
            map_view_request(ViewRequest::GitOpen {
                inventory_id: "inv".into(),
                item_id: "item".into(),
            }),
            RemoteWorkspaceViewRequest::GitOpen {
                inventory_id,
                item_id
            } if inventory_id == "inv" && item_id == "item"
        ));
        assert!(matches!(
            map_view_request(ViewRequest::File {
                path: "src/lib.rs".into(),
            }),
            RemoteWorkspaceViewRequest::File { path } if path == "src/lib.rs"
        ));
    }
}
