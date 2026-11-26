//! Offline source attachment. The original Project mutation/Prepared
//! owner retains all custody and publication authority.
use crate::git_maintenance::{
    open_private_approval, pin_named_layout, require_existing_store, require_named_destination,
    require_store_scope, validate_named_locations, PinnedGitLayout,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{CheckpointPolicy, DriverKey, SourceAnchor};
use aw_git::source_driver::{GitSourceAnchor, GitSourceDriver, GitSourceLocation};
use aw_host::source_checkpoint::GitCheckpointRestoreHost;
use aw_store::{SandboxAttachment, SandboxMaterializationRequest, SandboxSnapshot, SessionStore};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Existing private Store directory.
    #[arg(long = "state-dir")]
    state_dir: PathBuf,
    /// Private approval covering the entire named layout and incoming anchor.
    approval: PathBuf,
    /// Exact canonical unused Sandbox.
    sandbox: String,
    /// Stable original invocation identity.
    request_id: String,
    /// Inspect and publish the original prepared materialization.
    #[arg(long = "agent-recover", conflicts_with = "abandon")]
    recover: bool,
    /// Reconcile only the original unpublished incoming materialization.
    #[arg(long = "agent-abandon", conflicts_with = "recover")]
    abandon: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    sandbox: String,
    sandbox_rev: u64,
    anchor: SourceAnchor,
    destination_root: PathBuf,
    destination_identity: (u64, u64),
    repositories: Vec<GitSourceLocation>,
}

struct Approval {
    config: Config,
    directory: PathBuf,
    lease: File,
}

impl Approval {
    fn open(store: &SessionStore, actor: &PrincipalId, sandbox: &str, path: &Path) -> Result<Self> {
        let directory = path
            .parent()
            .ok_or(Error::Denied("attachment approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1
            || &config.manager != actor
            || config.sandbox != sandbox
            || config.sandbox_rev == 0
        {
            return Err(Error::Denied(
                "attachment approval differs from host manager or Sandbox",
            ));
        }
        require_store_scope(
            store,
            &config.store_directory,
            &config.store_id,
            config.store_generation,
        )?;
        require_named_destination(
            &config.destination_root,
            &config.destination_root,
            &config.store_directory,
            directory,
        )?;
        GitSourceAnchor::try_from(&config.anchor)?;
        if config.anchor.checkpoint_policy != CheckpointPolicy::Required {
            return Err(Error::Denied(
                "attachment requires an explicitly retained required Git version",
            ));
        }
        validate_named_locations(&mut config.repositories)?;
        if !config
            .repositories
            .iter()
            .any(|location| location.name == config.anchor.name)
        {
            return Err(Error::Denied("incoming source is not approved"));
        }
        Ok(Self {
            config,
            directory: directory.to_owned(),
            lease,
        })
    }

    fn request(&self) -> SandboxMaterializationRequest<'_> {
        SandboxMaterializationRequest {
            expected_rev: self.config.sandbox_rev,
            anchor: &self.config.anchor,
            replace: false,
        }
    }

    fn fence(&self, attachment: &SandboxAttachment, recover: bool) -> Result<PinnedGitLayout<'_>> {
        let c = &self.config;
        let snapshot = attachment.snapshot();
        if snapshot.canonical_ref != c.sandbox
            || snapshot.rev != c.sandbox_rev
            || snapshot.tool_root != c.destination_root
            || snapshot.working_directory != c.destination_root
            || attachment.anchor() != &c.anchor
            || attachment.replaced().is_some()
            || c.repositories.len() != attachment.bindings().len() + 1
        {
            return Err(Error::Denied(
                "attachment approval differs from the exact original layout or source",
            ));
        }
        // Check the full final named set, not just the incoming target.
        // Existing materialization identity and original provenance are
        // then verified by the same Git host that owns execution.
        let mut existing = attachment.bindings().iter();
        for location in &c.repositories {
            if location.name != c.anchor.name
                && !existing
                    .next()
                    .is_some_and(|binding| binding.binding.name == location.name)
            {
                return Err(Error::Denied("attachment approval omits a retained source"));
            }
        }
        if existing.next().is_some() {
            return Err(Error::Denied("attachment approval omits a retained source"));
        }
        let fence = pin_named_layout(
            &self.lease,
            &c.destination_root,
            c.destination_identity,
            &c.repositories,
            &[&c.store_directory, &self.directory],
            true,
        )?;
        // Before a new effect, no pre-existing incoming child can be
        // adopted. Recovery uses the original Prepared descriptor only.
        if !recover {
            match std::fs::symlink_metadata(c.destination_root.join(c.anchor.name.as_str())) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
                Ok(_) => {
                    return Err(Error::Conflict(
                        "incoming attachment destination already exists",
                    ))
                }
            }
        }
        Ok(fence)
    }
}

pub(crate) fn attach(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    request_id: &str,
    recover: bool,
) -> Result<SandboxSnapshot> {
    // Unlike standalone creation, attachment's exact incoming anchor and
    // original CAS revision are semantic request inputs. Read the approval
    // before replay, but never inspect a physical layout or driver on replay.
    let approval = Approval::open(store, actor, sandbox, path)?;
    let c = &approval.config;
    match store.sandbox_replay_attachment(actor, sandbox, c.sandbox_rev, request_id, &c.anchor) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) if recover => {
            return Err(Error::Denied("original prepared attachment unavailable"))
        }
        Ok(None) => (),
        Err(Error::Conflict(_)) if recover => (),
        Err(error) => return Err(error),
    }
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    let host = GitCheckpointRestoreHost::new(store, &handle);
    if recover {
        host.resume_sandbox_attachment_exact(
            actor,
            sandbox,
            request_id,
            approval.request(),
            |attachment, _| approval.fence(attachment, true),
        )
    } else {
        host.attach_sandbox_source(
            actor,
            sandbox,
            c.sandbox_rev,
            request_id,
            c.anchor.clone(),
            |attachment| approval.fence(attachment, false),
        )
    }
}

pub(crate) fn abandon(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    request_id: &str,
) -> Result<()> {
    let approval = Approval::open(store, actor, sandbox, path)?;
    if store.sandbox_replay_abandoned_attachment(actor, sandbox, request_id, approval.request())? {
        return Ok(());
    }
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    GitCheckpointRestoreHost::new(store, &handle).abandon_sandbox_attachment_exact(
        actor,
        sandbox,
        request_id,
        approval.request(),
        |attachment, _| approval.fence(attachment, true),
    )
}

pub(crate) fn run(args: Args) -> Result<u8> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    require_existing_store(&args.state_dir)?;
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;
    let id = format!("cli:sandbox-source-attach:{}", args.request_id);
    if args.abandon {
        abandon(&store, &actor, &args.approval, &args.sandbox, &id)?;
        super::print_json(&serde_json::json!({"status":"rejected"}))?;
    } else {
        let result = attach(
            &store,
            &actor,
            &args.approval,
            &args.sandbox,
            &id,
            args.recover,
        )?;
        super::print_json(&super::sandbox_snapshot_value(&result, true))?;
    }
    Ok(0)
}
