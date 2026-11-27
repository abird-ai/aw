//! Offline captured-source restore under an independently selected Git
//! administrative and destination-layout approval. Never activates an Agent.
use crate::git_maintenance::{
    open_private_approval, pin_named_layout, require_existing_store, require_named_destination,
    require_store_scope, validate_named_locations, PinnedGitLayout,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{DriverKey, SourceSet};
use aw_git::source_driver::{GitSourceAnchor, GitSourceDriver, GitSourceLocation};
use aw_host::source_checkpoint::GitCheckpointRestoreHost;
use aw_store::{
    AgentHistoryForkAccess, CheckpointRestoreRequest, CheckpointRestoreResult, SessionBinding,
    SessionStore,
};
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Existing private Store directory.
    #[arg(long = "state-dir")]
    state_dir: PathBuf,
    /// Private Git approval file, or the original empty baseline directory
    /// when --agent-baseline-controller is selected. The operator excludes
    /// other writers, aliases, retention users and destination access.
    approval: PathBuf,
    /// Exact checkpoint ID.
    checkpoint: String,
    /// Stable original invocation identity (retain for recovery).
    request_id: String,
    /// Optional destination Agent title, part of the original request.
    #[arg(long = "session-title")]
    title: Option<String>,
    /// Exact canonical destination Project; must match the private approval.
    /// Omit for the checkpoint source Project.
    #[arg(long = "agent-project")]
    project: Option<String>,
    /// Read the optional exact initial task from stdin (UTF-8, up to 64 KiB).
    /// For recovery/abandonment supply the original task bytes, not a new task.
    /// A restored source-bearing Agent remains held; this never runs the task.
    #[arg(long = "agent-task-stdin")]
    task_stdin: bool,
    /// Exact canonical controller Agent for empty-baseline RPC recovery or
    /// abandonment. Use its saved Session template and original baseline root.
    /// Requires the original task from stdin; never starts a new restore.
    #[arg(long = "agent-baseline-controller", requires_all = ["task_stdin", "rpc_request"])]
    baseline_controller: Option<String>,
    /// Inspect/publish the original prepared restore; never rematerialize.
    #[arg(long = "agent-recover", conflicts_with = "abandon")]
    recover: bool,
    /// Reconcile the original unpublished worktrees, never a published Agent.
    #[arg(long = "agent-abandon", conflicts_with = "recover")]
    abandon: bool,
    /// Inspect the original authenticated stdio RPC request ID instead of
    /// the standalone CLI namespace. Requires recovery or abandonment.
    #[arg(long = "rpc-request")]
    rpc_request: bool,
}

// Separate from native filesystem access or retired-retention permission:
// this selects the complete protected destination and original Git backing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    checkpoint: String,
    // Absence retains the original source-Project-only approval semantics.
    // An explicit selection authorizes this exact destination and no other.
    #[serde(default)]
    destination_project: Option<String>,
    binding: SessionBinding,
    destination_identity: (u64, u64),
    repositories: Vec<GitSourceLocation>,
}

struct Approval {
    config: Config,
    directory: PathBuf,
    _lease: File,
}

impl Approval {
    fn open(
        store: &SessionStore,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        path: &Path,
    ) -> Result<Self> {
        let directory = path
            .parent()
            .ok_or(Error::Denied("restore approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1
            || &config.manager != actor
            || config.checkpoint != request.checkpoint
            || config.destination_project != request.project
        {
            return Err(Error::Denied(
                "restore approval differs from host manager, checkpoint or destination Project",
            ));
        }
        require_store_scope(
            store,
            &config.store_directory,
            &config.store_id,
            config.store_generation,
        )?;
        // First cross-Project host cut: one Domain and Store, private
        // destination. History Share, destination writer/owner, and exact
        // canonical Project validation remain with the original Store
        // constructor before planning and again at publication.
        if let Some(project) = &config.destination_project {
            let checkpoint = store.checkpoint_get_metadata(actor, &request.checkpoint)?;
            let domain = checkpoint
                .source_agent_ref
                .split_once("/i:")
                .map(|(d, _)| d);
            if domain.is_none() || project.split_once("/i:").map(|(d, _)| d) != domain {
                return Err(Error::Denied("cross-Domain captured restore unavailable"));
            }
        }
        require_named_destination(
            &config.binding.tool_root,
            &config.binding.working_directory,
            &config.store_directory,
            directory,
        )?;
        validate_named_locations(&mut config.repositories)?;
        if config.repositories.is_empty() {
            return Err(Error::Denied(
                "captured restore requires approved Git sources",
            ));
        }
        Ok(Self {
            config,
            directory: directory.to_owned(),
            _lease: lease,
        })
    }

    fn fence(
        &self,
        layout: &SessionBinding,
        anchors: &SourceSet,
        recover: bool,
    ) -> Result<PinnedGitLayout<'_>> {
        let c = &self.config;
        if layout != &c.binding
            || c.repositories.len() != anchors.anchors.len()
            || anchors.anchors.is_empty()
        {
            return Err(Error::Denied(
                "restore approval differs from exact captured source set",
            ));
        }
        // Check the whole original anchor set before probing physical paths.
        for (anchor, location) in anchors.anchors.iter().zip(&c.repositories) {
            let anchor = GitSourceAnchor::try_from(anchor)?;
            if anchor.name != location.name || anchor.checkpoint_id != c.checkpoint {
                return Err(Error::Denied(
                    "restore approval names another captured source",
                ));
            }
        }
        pin_named_layout(
            &self._lease,
            &layout.tool_root,
            c.destination_identity,
            &c.repositories,
            &[&c.store_directory, &self.directory],
            recover,
        )
    }
}

fn require_offline_request(request: &CheckpointRestoreRequest) -> Result<()> {
    if request.access != AgentHistoryForkAccess::Private {
        return Err(Error::Denied(
            "offline captured restore requires private construction",
        ));
    }
    Ok(())
}

pub(crate) fn restore(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    request: &CheckpointRestoreRequest,
    request_id: &str,
    recover: bool,
) -> Result<CheckpointRestoreResult> {
    require_offline_request(request)?;
    // Applied replay requires neither physical approval nor Git lookup.
    // Incomplete operations never become new invocations.
    match store.checkpoint_replay_restore(actor, request, request_id) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) if recover => return Err(Error::Denied("original prepared restore unavailable")),
        Ok(None) => (),
        Err(Error::Conflict(_)) if recover => (), // exact original owner checks again
        Err(error) => return Err(error),
    }
    let approval = Approval::open(store, actor, request, path)?;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    let host = GitCheckpointRestoreHost::new(store, &handle);
    if recover {
        host.resume_captured_exact(
            actor,
            request,
            request_id,
            &approval.config.binding,
            |layout, anchors, _| approval.fence(layout, anchors, true),
        )
    } else {
        host.restore(
            actor,
            request,
            request_id,
            "local",
            || Ok(approval.config.binding.clone()),
            |layout, anchors| approval.fence(layout, anchors, false),
        )
    }
}

/// Exact offline abandonment; no new layout, task, source or published
/// Agent cleanup. A terminal Rejected replay does not reopen host state.
pub(crate) fn abandon(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    request: &CheckpointRestoreRequest,
    request_id: &str,
) -> Result<()> {
    require_offline_request(request)?;
    if store.checkpoint_replay_abandoned_captured_restore(actor, request, request_id)? {
        return Ok(());
    }
    let approval = Approval::open(store, actor, request, path)?;
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    GitCheckpointRestoreHost::new(store, &handle).abandon_captured_exact(
        actor,
        request,
        request_id,
        &approval.config.binding,
        |anchors, _| approval.fence(&approval.config.binding, anchors, true),
    )
}

// Preserve exact task bytes for the original semantic fingerprint. Do not
// trim newlines, accept an unbounded stream, or put task text in argv.
fn read_task(input: impl Read) -> Result<String> {
    const MAX_TASK_BYTES: usize = 64 * 1024;
    let mut bytes = Vec::new();
    input
        .take((MAX_TASK_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > MAX_TASK_BYTES || bytes.contains(&0) {
        return Err(Error::Limit(
            "initial task must contain 1..65536 safe bytes",
        ));
    }
    String::from_utf8(bytes).map_err(|_| Error::Protocol("initial task must be UTF-8"))
}

pub(crate) fn run(args: Args) -> Result<u8> {
    let request_id = crate::source_construction_rpc::Operation::CheckpointRestore.offline_id(
        &args.request_id,
        args.rpc_request,
        args.recover || args.abandon,
    )?;
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    require_existing_store(&args.state_dir)?;
    // Do not hold the exclusive Store owner lock while waiting for input.
    let task = args
        .task_stdin
        .then(|| read_task(std::io::stdin().lock()))
        .transpose()?;
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;
    let request = CheckpointRestoreRequest {
        checkpoint: args.checkpoint,
        project: args.project,
        title: args.title,
        access: AgentHistoryForkAccess::Private,
        task,
    };

    let result = if let Some(controller) = args.baseline_controller {
        // The RPC constructor selected its template from the current saved
        // Session. Recover from that durable binding, not today's provider
        // configuration or a caller-supplied layout template.
        let snapshot = store.agent_get(&actor, &controller)?;
        let execution = store.agent_execution_binding(&actor, &controller)?;
        let template = store.session(&actor, execution.session_id())?.binding;
        let baseline = aw_host::checkpoint_baseline::EmptyCheckpointRestore::from_trusted_host(
            store,
            actor,
            snapshot.project_ref,
            template,
            args.approval,
        );
        if args.abandon {
            baseline.abandon(&request, &request_id)?;
            super::print_json(&serde_json::json!({"status":"rejected"}))?;
            return Ok(0);
        }
        baseline.resume(&request, &request_id)?
    } else {
        if args.abandon {
            abandon(&store, &actor, &args.approval, &request, &request_id)?;
            super::print_json(&serde_json::json!({"status":"rejected"}))?;
            return Ok(0);
        }
        restore(
            &store,
            &actor,
            &args.approval,
            &request,
            &request_id,
            args.recover,
        )?
    };
    super::print_json(&serde_json::json!({"ref":result.agent.canonical_ref,
        "sandbox_ref":result.agent.sandbox_ref,"admission":result.agent.admission,
        "source_coverage":result.source_coverage}))?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::read_task;

    #[test]
    fn task_stdin_is_bounded_and_preserves_exact_utf8() {
        let task = "Read the captured source — fixture task.\n";
        assert_eq!(read_task(task.as_bytes()).unwrap(), task);
        assert!(read_task(&[][..]).is_err());
        assert!(read_task(&[0][..]).is_err());
        assert!(read_task(&[0xff][..]).is_err());
        assert!(read_task(&vec![b'x'; 64 * 1024 + 1][..]).is_err());
        assert_eq!(
            read_task(&vec![b'x'; 64 * 1024][..]).unwrap().len(),
            64 * 1024
        );
    }
}
