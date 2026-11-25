//! Explicit offline Git checkpoint capture. The configured native approval
//! is only the source selection. Invoking this separate trusted operator
//! command authorizes the exact Git administrative effect, not Agent tools.
use crate::git_maintenance::{
    canonical_private_directory, lock_private_approval, overlaps, pin_private_checkout,
    require_existing_store,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{DriverKey, SourceName};
use aw_git::source_driver::{GitSourceDriver, GitSourceLocation};
use aw_host::{
    source_approval::{NativeSourceApproval, NativeSourceApprovalGuard},
    source_checkpoint::{GitCheckpointCaptureHost, GitSourceFence},
    source_git::GitNativeSourceResolver,
};
use aw_platform::DirectoryIdentity;
use aw_store::{
    CheckpointCaptureRequest, CheckpointCaptureSelection, CheckpointSnapshot, SessionStore,
    VersionedSourceBinding,
};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Existing private Store directory.
    #[arg(long = "state-dir")]
    state_dir: PathBuf,
    /// Canonical private directory containing git-native.json.
    /// The operator independently authorizes Git administration and excludes
    /// all external writers, aliases and other users through settlement.
    approval_root: PathBuf,
    /// Exact canonical Agent, not a contextual alias.
    agent: String,
    /// Stable original invocation identity (retain for recovery).
    request_id: String,
    /// Select named sources (default: all required sources).
    #[arg(long = "agent-source")]
    sources: Vec<String>,
    /// Optional checkpoint note, part of the original request semantics.
    #[arg(long = "agent-note")]
    note: Option<String>,
    /// Inspect/publish the exact original prepared capture; never recapture.
    #[arg(long = "agent-recover")]
    recover: bool,
}

struct Fence {
    _guard: NativeSourceApprovalGuard,
    _approval: NativeSourceApproval,
    locations: Vec<GitSourceLocation>,
    _pins: Vec<DirectoryIdentity>,
    _lease: File,
}

impl GitSourceFence for Fence {
    fn locations(&self) -> &[GitSourceLocation] {
        &self.locations
    }
}

fn authorize_capture(
    store: &SessionStore,
    resolver: &GitNativeSourceResolver,
    actor: &PrincipalId,
    reference: &str,
    root: &Path,
    bindings: &[VersionedSourceBinding],
) -> Result<Fence> {
    canonical_private_directory(root)?;
    let state = store
        .directory()
        .ok_or(Error::Denied("capture requires durable state"))?;
    if overlaps(root, state)
        || DirectoryIdentity::open(root)?.identity()?
            == DirectoryIdentity::open(state)?.identity()?
    {
        return Err(Error::Denied("capture approval overlaps Store state"));
    }
    let lease = lock_private_approval(&root.join("git-native.json"))?;
    // Explicit offline host authority includes Git administration; native
    // read/write ceilings alone do not grant retention mutation. Check the
    // exact selected binding set before any physical probes or provisioning.
    let (approval, (locations, pins)) =
        resolver.provision_configured_approval_with(actor, root, |_, spec, current| {
            if &spec.actor != actor || spec.agent_ref != reference || current != bindings {
                return Err(Error::Denied(
                    "capture approval differs from exact original Agent or sources",
                ));
            }
            let mut pins = Vec::with_capacity(spec.directories.len() * 3);
            let locations = spec
                .directories
                .iter()
                .map(|directory| {
                    pins.extend(pin_private_checkout(&directory.location, &[state, root])?);
                    Ok(directory.location.clone())
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((locations, pins))
        })?;
    let guard = approval.read()?;
    Ok(Fence {
        _guard: guard,
        _approval: approval,
        locations,
        _pins: pins,
        _lease: lease,
    })
}

pub(crate) fn capture(
    store: &SessionStore,
    actor: &PrincipalId,
    root: &Path,
    request: &CheckpointCaptureRequest<'_>,
    request_id: &str,
    recover: bool,
) -> Result<CheckpointSnapshot> {
    // Applied replay must not reload private config, resolve a driver or
    // follow a physical path. An unresolved invocation is never fresh work.
    match store.checkpoint_replay_capture(
        actor,
        request.reference,
        request.selection.clone(),
        request.note,
        request_id,
    ) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) if recover => return Err(Error::Denied("original prepared capture unavailable")),
        Ok(None) => (),
        Err(Error::Conflict(_)) if recover => (), // exact receipt checked again by recovery owner
        Err(error) => return Err(error),
    }
    let handle = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
    let resolver = GitNativeSourceResolver::new(store.clone(), handle.clone())?;
    let host = GitCheckpointCaptureHost::new(store, &handle);
    let authorize = |bindings: &[VersionedSourceBinding]| {
        authorize_capture(store, &resolver, actor, request.reference, root, bindings)
    };
    if recover {
        host.resume_exact(actor, request, request_id, |bindings, _| {
            authorize(bindings)
        })
    } else {
        host.capture(
            actor,
            request.reference,
            request.selection.clone(),
            request.note,
            request_id,
            authorize,
        )
    }
}

pub(crate) fn run(args: Args) -> Result<u8> {
    let actor = PrincipalId::new(aw_platform::local_principal()?)?;
    require_existing_store(&args.state_dir)?;
    let selection = if args.sources.is_empty() {
        CheckpointCaptureSelection::Auto
    } else {
        CheckpointCaptureSelection::Named(
            args.sources
                .into_iter()
                .map(SourceName::new)
                .collect::<Result<Vec<_>>>()?,
        )
    };
    let request = CheckpointCaptureRequest {
        reference: &args.agent,
        selection,
        note: args.note.as_deref(),
    };
    let store = aw::sessions::open_sqlite_owner(&args.state_dir)?;
    let snapshot = capture(
        &store,
        &actor,
        &args.approval_root,
        &request,
        &format!("cli:checkpoint-capture:{}", args.request_id),
        args.recover,
    )?;
    super::print_json(
        &serde_json::json!({"id":snapshot.id, "lifecycle":snapshot.lifecycle, "source_coverage":snapshot.source_coverage}),
    )?;
    Ok(0)
}
