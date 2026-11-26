//! Offline source materialization (attachment or replacement). The original Project mutation/Prepared
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
    /// Reconcile the original unpublished candidate; replacement requires an intact predecessor.
    #[arg(long = "agent-abandon", conflicts_with = "recover")]
    abandon: bool,
}

#[derive(clap::Args, Debug)]
pub(crate) struct ReplaceArgs {
    #[command(flatten)]
    common: Args,
    /// Explicitly continue the original post-release gap under renewed authority.
    #[arg(long = "agent-continue", conflicts_with_all = ["recover", "abandon"])]
    continue_effect: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterializationMode {
    Fresh,
    Recover,
    Continue,
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
    /// Explicit independent disposal approval of the old origin, at the
    /// same named destination as the incoming version.
    #[serde(default)]
    replaced_repository: Option<GitSourceLocation>,
    /// Separately approve exact continuation, not general retry.
    #[serde(default)]
    allow_continuation: bool,
}

struct Approval {
    config: Config,
    directory: PathBuf,
    lease: File,
}

impl Approval {
    fn open(
        store: &SessionStore,
        actor: &PrincipalId,
        sandbox: &str,
        path: &Path,
        replace: bool,
    ) -> Result<Self> {
        let directory = path
            .parent()
            .ok_or(Error::Denied("attachment approval directory required"))?;
        let (lease, mut config): (_, Config) = open_private_approval(path)?;
        if config.version != 1
            || &config.manager != actor
            || config.sandbox != sandbox
            || config.sandbox_rev == 0
            || config.replaced_repository.is_some() != replace
            || (!replace && config.allow_continuation)
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
        if let Some(old) = &mut config.replaced_repository {
            validate_named_locations(std::slice::from_mut(old))?;
            if old.name != config.anchor.name {
                return Err(Error::Denied(
                    "replacement approval names another old source",
                ));
            }
        }
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
            replace: self.config.replaced_repository.is_some(),
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
            || attachment.replaced().is_some() != c.replaced_repository.is_some()
            || c.repositories.len()
                != attachment.bindings().len() + usize::from(c.replaced_repository.is_none())
        {
            return Err(Error::Denied(
                "attachment approval differs from the exact original layout or source",
            ));
        }
        // Check the full final named set, not just the incoming target.
        // Existing materialization identity and original provenance are
        // then verified by the same Git host that owns execution.
        let mut existing = attachment
            .bindings()
            .iter()
            .filter(|binding| binding.binding.name != c.anchor.name);
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
        let protected = [c.store_directory.as_path(), self.directory.as_path()];
        let mut fence = pin_named_layout(
            &self.lease,
            &c.destination_root,
            c.destination_identity,
            &c.repositories,
            &protected,
            true,
        )?;
        if let Some(old) = &c.replaced_repository {
            fence.pin_replaced_origin(old, &protected)?;
        }
        // Before a new attachment, no pre-existing incoming child can
        // be adopted. Replacement instead proves its exact predecessor
        // using the original Applied descriptor in the Git host.
        if !recover && c.replaced_repository.is_none() {
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
    materialize(
        store,
        actor,
        path,
        sandbox,
        request_id,
        false,
        if recover {
            MaterializationMode::Recover
        } else {
            MaterializationMode::Fresh
        },
    )
}

pub(crate) fn replace(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    request_id: &str,
    mode: MaterializationMode,
) -> Result<SandboxSnapshot> {
    materialize(store, actor, path, sandbox, request_id, true, mode)
}

fn materialize(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    request_id: &str,
    replace: bool,
    mode: MaterializationMode,
) -> Result<SandboxSnapshot> {
    // Unlike standalone creation, the exact anchor and original CAS
    // revision are semantic request inputs. Read the approval before
    // replay, but never inspect a physical layout or driver on replay.
    let approval = Approval::open(store, actor, sandbox, path, replace)?;
    let c = &approval.config;
    let replay = if replace {
        store.sandbox_replay_replacement(actor, sandbox, c.sandbox_rev, request_id, &c.anchor)
    } else {
        store.sandbox_replay_attachment(actor, sandbox, c.sandbox_rev, request_id, &c.anchor)
    };
    let recover = mode != MaterializationMode::Fresh;
    match replay {
        Ok(Some(result)) => return Ok(result),
        Ok(None) if recover => {
            return Err(Error::Denied(
                "original prepared materialization unavailable",
            ))
        }
        Ok(None) => (),
        Err(Error::Conflict(_)) if recover => (),
        Err(error) => return Err(error),
    }
    if mode == MaterializationMode::Continue && (!replace || !c.allow_continuation) {
        return Err(Error::Denied(
            "replacement continuation requires explicit host approval",
        ));
    }
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    let host = GitCheckpointRestoreHost::new(store, &handle);
    match mode {
        MaterializationMode::Recover => host.resume_sandbox_attachment_exact(
            actor,
            sandbox,
            request_id,
            approval.request(),
            |attachment, _| approval.fence(attachment, true),
        ),
        MaterializationMode::Continue => host.continue_sandbox_replacement_exact(
            actor,
            sandbox,
            request_id,
            approval.request(),
            |attachment, _| approval.fence(attachment, true),
        ),
        MaterializationMode::Fresh if replace => host.replace_sandbox_source(
            actor,
            sandbox,
            c.sandbox_rev,
            request_id,
            c.anchor.clone(),
            |attachment| approval.fence(attachment, false),
        ),
        MaterializationMode::Fresh => host.attach_sandbox_source(
            actor,
            sandbox,
            c.sandbox_rev,
            request_id,
            c.anchor.clone(),
            |attachment| approval.fence(attachment, false),
        ),
    }
}

pub(crate) fn abandon(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    request_id: &str,
) -> Result<()> {
    abandon_in(store, actor, path, sandbox, request_id, false)
}

pub(crate) fn abandon_replacement(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    request_id: &str,
) -> Result<()> {
    abandon_in(store, actor, path, sandbox, request_id, true)
}

fn abandon_in(
    store: &SessionStore,
    actor: &PrincipalId,
    path: &Path,
    sandbox: &str,
    request_id: &str,
    replace: bool,
) -> Result<()> {
    let approval = Approval::open(store, actor, sandbox, path, replace)?;
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
    run_in(args, false, false)
}

pub(crate) fn run_replace(args: ReplaceArgs) -> Result<u8> {
    run_in(args.common, true, args.continue_effect)
}

fn run_in(args: Args, replacing: bool, continuing: bool) -> Result<u8> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    require_existing_store(&args.state_dir)?;
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;
    let operation = if replacing { "replace" } else { "attach" };
    let id = format!("cli:sandbox-source-{operation}:{}", args.request_id);
    if args.abandon {
        if replacing {
            abandon_replacement(&store, &actor, &args.approval, &args.sandbox, &id)?;
        } else {
            abandon(&store, &actor, &args.approval, &args.sandbox, &id)?;
        }
        super::print_json(&serde_json::json!({"status":"rejected"}))?;
    } else {
        let result = if replacing {
            let mode = if continuing {
                MaterializationMode::Continue
            } else if args.recover {
                MaterializationMode::Recover
            } else {
                MaterializationMode::Fresh
            };
            replace(&store, &actor, &args.approval, &args.sandbox, &id, mode)?
        } else {
            attach(
                &store,
                &actor,
                &args.approval,
                &args.sandbox,
                &id,
                args.recover,
            )?
        };
        super::print_json(&super::sandbox_snapshot_value(&result, true))?;
    }
    Ok(0)
}
