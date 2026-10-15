use super::machinelink::{MachineCapability, MachineLinkConnectOptions};
use super::remote_sessions::{
    create_remote_session, RemoteSessionCreateOptions, RemoteSessionCreateResult,
};
use super::*;
use aw::PrincipalId;
use aw_store::{FleetOperationSnapshot, WorkspaceInfo};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct FleetMemberDispatchOptions {
    pub actor: PrincipalId,
    pub operation_id: String,
    pub machine_id: String,
    pub link: MachineLinkConnectOptions,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FleetMemberDispatchResult {
    pub operation_id: String,
    pub machine_id: String,
    pub remote_operation_id: String,
    pub remote: RemoteSessionCreateResult,
    pub fleet: FleetOperationSnapshot,
}

fn fleet_remote_operation_id(operation_id: &str, machine_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"aw.fleet.remote-session.v1\0");
    digest.update(operation_id.as_bytes());
    digest.update(b"\0");
    digest.update(machine_id.as_bytes());
    format!("fro_{}", hex(&digest.finalize()))
}

fn source_workspace(
    store: &SessionStore,
    actor: &PrincipalId,
    snapshot: &FleetOperationSnapshot,
) -> Result<WorkspaceInfo> {
    let workspace_id = snapshot
        .operation
        .source_workspace_id
        .as_deref()
        .ok_or_else(|| {
            Error::Config(
                "Fleet dispatch requires the Phase 20 operation to pin a source Workspace".into(),
            )
        })?;
    let fleet = store.fleet(actor, &snapshot.operation.fleet_id)?;
    store.workspace(actor, Some(&fleet.domain_id), workspace_id)
}

pub fn dispatch_fleet_member_session(
    store: &SessionStore,
    state_dir: &Path,
    mut opts: FleetMemberDispatchOptions,
) -> Result<FleetMemberDispatchResult> {
    if !opts
        .link
        .requested
        .contains(&MachineCapability::RemoteSession)
    {
        return Err(Error::Config(
            "Fleet member dispatch must explicitly request remote-session".into(),
        ));
    }

    // This is the authoritative Phase 20 membership snapshot. Never call
    // fleet_members() here: retries must target the original exact member set.
    let snapshot = store.fleet_operation(&opts.actor, &opts.operation_id)?;
    let member = snapshot
        .members
        .iter()
        .find(|member| member.machine_id == opts.machine_id)
        .ok_or(Error::Denied(
            "machine is not part of this pinned Fleet operation",
        ))?
        .clone();

    if matches!(member.state.as_str(), "completed" | "skipped" | "failed") {
        return Err(Error::Conflict(
            "Fleet member is already terminal; create an explicit new operation to rerun it",
        ));
    }
    if member.state == "active" {
        return Err(Error::Conflict(
            "Fleet member already has an active remote Session; supervise that Session instead",
        ));
    }

    let fleet = store.fleet(&opts.actor, &snapshot.operation.fleet_id)?;
    let machine = store.machine(&opts.actor, Some(&fleet.domain_id), &opts.machine_id)?;
    if machine.node_id != opts.link.route.peer_node_id {
        return Err(Error::Denied(
            "Fleet dispatch route does not match the pinned member Machine identity",
        ));
    }
    let workspace = source_workspace(store, &opts.actor, &snapshot)?;
    let remote_operation_id =
        fleet_remote_operation_id(&snapshot.operation.operation_id, &member.machine_id);

    // Journal a conservative unknown member state before opening the network
    // effect boundary. If the call fails at any later point, retry uses the
    // deterministic remote operation ID and the same Phase 20 member row.
    store.update_fleet_operation_member(
        &opts.actor,
        &snapshot.operation.operation_id,
        &member.machine_id,
        "unknown",
        member.remote_ref_id.as_deref(),
        member.result_sha256.as_deref(),
    )?;

    let title = opts.title.take().unwrap_or_else(|| {
        format!(
            "Fleet {} / {}",
            snapshot.operation.operation_id, member.machine_id
        )
    });
    let remote = create_remote_session(
        store,
        state_dir,
        RemoteSessionCreateOptions {
            link: opts.link,
            actor: opts.actor.clone(),
            source_workspace: workspace,
            source_session_id: None,
            source_branch_id: None,
            title,
            operation_id: Some(remote_operation_id.clone()),
        },
    )?;

    let fleet = store.update_fleet_operation_member(
        &opts.actor,
        &snapshot.operation.operation_id,
        &member.machine_id,
        "active",
        Some(&remote.reference.id),
        None,
    )?;

    Ok(FleetMemberDispatchResult {
        operation_id: snapshot.operation.operation_id,
        machine_id: member.machine_id,
        remote_operation_id,
        remote,
        fleet,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_remote_operation_ids_are_member_specific_and_stable() {
        let a = fleet_remote_operation_id("op", "m1");
        assert_eq!(a, fleet_remote_operation_id("op", "m1"));
        assert_ne!(a, fleet_remote_operation_id("op", "m2"));
        assert!(a.starts_with("fro_"));
        assert_eq!(a.len(), 68);
    }
}
