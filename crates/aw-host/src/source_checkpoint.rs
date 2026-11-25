//! Concrete Git checkpoint lifecycle. Store owns Prepared custody,
//! recovery, history and final publication; the host owns physical authority.
//! Blocking trusted-host operations, not an Actor/model/RPC admission route.
use aw_agent_core::{
    Error, ExternalEvidence, NamedPreparedSource, PreparedSourceSet, PrincipalId,
    RecoveryInspection, Result, SourceBinding, SourceDriverHandle, SourceOperationKind,
    SourcePublicationFence, SourcePublicationProof, SourceSet,
};
use aw_git::source_driver::{GitRestoreLocation, GitSourceAnchor, GitSourceDriver};
use aw_store::{
    CheckpointRestoreLayout, CheckpointRestoreRequest, CheckpointRestoreResult,
    InspectedCheckpointRestore, SandboxSnapshot, SessionBinding, SessionStore,
    VersionedSourceBinding,
};
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path},
};

mod capture;
pub use capture::{GitCheckpointCaptureHost, GitSourceFence};

/// Live, independently authorized, whole-layout physical/provider fence.
///
/// The trusted authority owner must guarantee current actor/source disclosure,
/// repository administration and retention, destination isolation (including
/// aliases and other Stores), and the permitted restore or cleanup effect until
/// this guard drops. Retirement additionally requires authority over the entire
/// retained execution layout, including any external physical dependencies.
/// Scratch and partial-restore recovery require the original private candidate
/// and reserved Git artifact namespace to have remained protected and unpublished,
/// with no surviving writer or unaccounted alias.
/// Locations are immutable, in canonical source-name order.
/// Neither a path pin, Git worktree lock nor a saved evidence handle suffices.
/// Acquisition before Prepared must be read-only, not a materializing effect.
pub trait GitRestoreFence {
    fn locations(&self) -> &[GitRestoreLocation];
}

// Borrow the actual whole-set guard through the Store publication transaction.
// The inspected identity labels its exact checked result; it grants nothing.
struct Publication<'a, F> {
    _fence: &'a F,
    evidence: ExternalEvidence,
}

impl<F> SourcePublicationFence for Publication<'_, F> {
    fn evidence(&self) -> &ExternalEvidence {
        &self.evidence
    }
}

fn approved_anchors(
    anchors: &SourceSet,
    fence: &impl GitRestoreFence,
) -> Result<Vec<GitSourceAnchor>> {
    let locations = fence.locations();
    if anchors.anchors.len() != locations.len()
        || anchors
            .anchors
            .iter()
            .zip(locations)
            .any(|(anchor, location)| anchor.name != location.name)
    {
        return Err(Error::Denied(
            "Git restore approval must cover the exact named SourceSet",
        ));
    }
    require_disjoint_locations(locations)?;
    anchors
        .anchors
        .iter()
        .map(GitSourceAnchor::try_from)
        .collect()
}

fn require_disjoint_locations(locations: &[GitRestoreLocation]) -> Result<()> {
    // Cheap lexical checks in addition to (not in place of) the host's
    // physical alias/administration/whole-layout isolation guarantee.
    for (i, location) in locations.iter().enumerate() {
        for other in &locations[i + 1..] {
            if location.target.starts_with(&other.target)
                || other.target.starts_with(&location.target)
            {
                return Err(Error::Denied("Git restore destinations overlap"));
            }
        }
        if locations.iter().any(|other| {
            location.target.starts_with(&other.repository)
                || other.repository.starts_with(&location.target)
        }) {
            return Err(Error::Denied("Git restore destination overlaps an origin"));
        }
    }
    Ok(())
}

/// Only layouts whose complete release can be proven without deleting any
/// additional user or host files. The physical fence must exclude outside
/// users, aliases and mounts, not merely hold the directory open.
enum ManagedGitLayout<'a> {
    Checkout(&'a Path),
    Directory(&'a Path),
}

impl<'a> ManagedGitLayout<'a> {
    fn check(sandbox: &'a SandboxSnapshot, locations: &[GitRestoreLocation]) -> Result<Self> {
        let root = sandbox.tool_root.as_path();
        let cwd = sandbox
            .working_directory
            .strip_prefix(root)
            .map_err(|_| Error::Denied("Git retirement requires cwd inside the managed layout"))?;
        if cwd
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(Error::Denied(
                "Git retirement requires a contained working directory",
            ));
        }
        if let [location] = locations {
            if location.target == root {
                return Ok(Self::Checkout(root));
            }
        }
        if locations.is_empty()
            || locations
                .iter()
                .any(|location| location.target.parent() != Some(root))
            || root.canonicalize()? != root
        {
            return Err(Error::Denied("unsupported managed Git retirement layout"));
        }
        // The approved parent was already identity-checked by the driver.
        // Refuse foreign entries before deleting any managed checkout.
        let targets = locations
            .iter()
            .map(|location| location.target.as_path())
            .collect::<HashSet<_>>();
        for entry in fs::read_dir(root)? {
            if !targets.contains(entry?.path().as_path()) {
                return Err(Error::Conflict(
                    "managed Git layout contains unrelated files",
                ));
            }
        }
        Ok(Self::Directory(root))
    }

    fn require_released(self) -> Result<()> {
        match self {
            Self::Checkout(root) => match fs::symlink_metadata(root) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
                Ok(_) => Err(Error::Conflict("managed Git checkout remains present")),
            },
            Self::Directory(root) => {
                if root.canonicalize()? != root || fs::read_dir(root)?.next().transpose()?.is_some()
                {
                    return Err(Error::Conflict("managed Git layout is not empty"));
                }
                Ok(())
            }
        }
    }
}

fn plan(
    driver: &GitSourceDriver,
    anchors: &SourceSet,
    fence: &impl GitRestoreFence,
    prepare_id: &str,
) -> Result<PreparedSourceSet> {
    let anchors = approved_anchors(anchors, fence)?;
    let sources = anchors
        .iter()
        .zip(fence.locations())
        .map(|(anchor, location)| {
            Ok(NamedPreparedSource {
                name: anchor.name.clone(),
                prepared: driver.plan_restore_approved(location, anchor, prepare_id)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(PreparedSourceSet {
        prepare_id: prepare_id.to_owned(),
        operation: SourceOperationKind::Restore,
        sources,
    })
}

fn require_locations(
    driver: &GitSourceDriver,
    anchors: &SourceSet,
    prepared: &PreparedSourceSet,
    fence: &impl GitRestoreFence,
) -> Result<()> {
    let anchors = approved_anchors(anchors, fence)?;
    if prepared.sources.len() != anchors.len() {
        return Err(Error::Conflict(
            "Git restore preparation differs from approved SourceSet",
        ));
    }
    for ((anchor, source), location) in anchors.iter().zip(&prepared.sources).zip(fence.locations())
    {
        if source.name != anchor.name {
            return Err(Error::Conflict(
                "Git restore preparation names another source",
            ));
        }
        driver.verify_restore_location(location, anchor, &source.prepared)?;
    }
    Ok(())
}

fn inspect(
    driver: &GitSourceDriver,
    anchors: &SourceSet,
    prepared: &PreparedSourceSet,
    fence: &impl GitRestoreFence,
) -> Result<Vec<RecoveryInspection<SourceBinding>>> {
    require_locations(driver, anchors, prepared, fence)?;
    prepared
        .sources
        .iter()
        .map(|source| driver.inspect_restore(&source.prepared))
        .collect()
}

/// Uses the actual SourceDriver registered in the Store; no second driver
/// registry, authority state, replay ledger or restore constructor.
pub struct GitCheckpointRestoreHost<'a> {
    store: &'a SessionStore,
    driver: &'a SourceDriverHandle<GitSourceDriver>,
}

impl<'a> GitCheckpointRestoreHost<'a> {
    pub fn new(store: &'a SessionStore, driver: &'a SourceDriverHandle<GitSourceDriver>) -> Self {
        Self { store, driver }
    }

    /// New captured-source restore, or exact Applied replay. Layout selection
    /// and independent approval are not consulted on replay. The host selects
    /// an inert layout and authorizes the complete named set before Git reads
    /// or effects; the returned live guard spans the actual Store commit.
    /// Baseline versions are selected explicitly by restore_versioned_baseline.
    /// Non-Git SourceSets require their own host route.
    pub fn restore<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        execution_site: &str,
        select_layout: impl FnOnce() -> Result<SessionBinding>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        self.restore_selected(
            actor,
            request,
            request_id,
            execution_site,
            || Ok((select_layout()?, None)),
            authorize,
        )
    }

    /// Restore history into a fresh, independently selected versioned
    /// destination baseline. Selection is read-only and occurs only for new
    /// work. The host owns retention of every chosen version and current
    /// destination policy; this never captures or alters the checkpoint.
    pub fn restore_versioned_baseline<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        execution_site: &str,
        select: impl FnOnce() -> Result<(SessionBinding, SourceSet)>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        self.restore_selected(
            actor,
            request,
            request_id,
            execution_site,
            || {
                let (layout, sources) = select()?;
                Ok((layout, Some(sources)))
            },
            authorize,
        )
    }

    fn restore_selected<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        execution_site: &str,
        select: impl FnOnce() -> Result<(SessionBinding, Option<SourceSet>)>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        if let Some(result) = self
            .store
            .checkpoint_replay_restore(actor, request, request_id)?
        {
            return Ok(result);
        }
        let (layout, baseline) = select()?;
        let driver = self.store.source_driver(self.driver)?;
        let mut fence = None;
        let prepared = self.store.checkpoint_prepare_restore_with_layout(
            actor,
            request,
            CheckpointRestoreLayout {
                binding: &layout,
                baseline,
            },
            request_id,
            execution_site,
            |prepare_id, _, anchors| {
                let approval = authorize(&layout, anchors)?;
                let sources = plan(&driver, anchors, &approval, prepare_id)?;
                fence = Some(approval);
                Ok(sources)
            },
        )?;
        let fence = fence.expect("successful preparation called planner");
        // All original descriptors and locations are checked before the
        // first Git mutation. An uncertain result never retries execution.
        let executed = (|| {
            prepared.revalidate(&layout)?;
            require_locations(&driver, prepared.anchors(), prepared.sources(), &fence)?;
            for source in &prepared.sources().sources {
                driver.execute_restore(&source.prepared)?;
            }
            Ok(())
        })();
        if let Err(error) = executed {
            prepared.mark_unknown()?;
            return Err(error);
        }
        let inspected = prepared.inspect(|registry, anchors, sources| {
            let driver = registry.get(self.driver)?;
            inspect(&driver, anchors, sources, &fence)
        })?;
        self.publish(inspected, &layout, &fence)
    }

    /// Explicit recovery of the original unresolved request. The host must
    /// freshly authorize the original descriptors and physical locations.
    /// Inspect only; never select a new materialization or execute again.
    pub fn resume<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        layout: &SessionBinding,
        authorize: impl FnOnce(&SessionBinding, &SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        let mut fence = None;
        let inspected = self.store.checkpoint_inspect_restore(
            actor,
            request_id,
            |registry, anchors, sources| {
                let approval = authorize(layout, anchors, sources)?;
                let driver = registry.get(self.driver)?;
                let result = inspect(&driver, anchors, sources, &approval)?;
                fence = Some(approval);
                Ok(result)
            },
        )?;
        self.publish(
            inspected,
            layout,
            &fence.expect("successful inspection called inspector"),
        )
    }

    fn publish<F: GitRestoreFence>(
        &self,
        inspected: InspectedCheckpointRestore<'_>,
        layout: &SessionBinding,
        fence: &F,
    ) -> Result<CheckpointRestoreResult> {
        inspected.publish(layout, |registry, anchors, sources, bindings, evidence| {
            let driver = registry.get(self.driver)?;
            require_locations(&driver, anchors, sources, fence)?;
            sources
                .sources
                .iter()
                .zip(bindings)
                .zip(evidence)
                .map(|((source, binding), evidence)| {
                    driver.revalidate_restore(&source.prepared, binding)?;
                    Ok(SourcePublicationProof::Held(Publication {
                        _fence: fence,
                        evidence: evidence.clone(),
                    }))
                })
                .collect()
        })
    }

    /// Explicitly abandon an original unpublished candidate, with independent
    /// cleanup authority. No broad prune, new descriptor or Applied cleanup.
    /// The Store retains the returned live guard through Rejected settlement.
    pub fn abandon<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request_id: &str,
        authorize: impl FnOnce(&SourceSet, &PreparedSourceSet) -> Result<F>,
    ) -> Result<()> {
        self.store
            .checkpoint_abandon_restore(actor, request_id, |registry, anchors, sources| {
                let fence = authorize(anchors, sources)?;
                let driver = registry.get(self.driver)?;
                require_locations(&driver, anchors, sources, &fence)?;
                for source in &sources.sources {
                    driver.release_prepared_restore(&source.prepared)?;
                }
                Ok(fence)
            })
    }

    /// Reconcile a closed, retained Sandbox created by a captured-Git
    /// restore. Only the original, unmodified binding set is supported.
    /// Authorize the whole layout and exact Applied descriptors independently.
    /// Keep the external fence through Store settlement; never remove shared
    /// backing or unrelated files. Already released layouts do not reauthorize.
    pub fn reconcile_closed_sandbox<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        sandbox: &str,
        authorize: impl FnOnce(
            &SandboxSnapshot,
            &[VersionedSourceBinding],
            &PreparedSourceSet,
        ) -> Result<F>,
    ) -> Result<SandboxSnapshot> {
        self.store
            .sandbox_reconcile_closed_sources(actor, sandbox, |snapshot, bindings| {
                let prepared = self
                    .store
                    .sandbox_restore_preparation(actor, &snapshot.canonical_ref)?;
                let fence = authorize(snapshot, bindings, &prepared)?;
                let locations = fence.locations();
                if prepared.sources.len() != bindings.len()
                    || bindings.len() != locations.len()
                    || bindings.iter().zip(&prepared.sources).zip(locations).any(
                        |((binding, source), location)| {
                            binding.binding_rev != 1
                                || binding.binding.name != source.name
                                || source.name != location.name
                        },
                    )
                {
                    return Err(Error::Conflict(
                        "retained Git bindings differ from the original restore",
                    ));
                }
                require_disjoint_locations(locations)?;
                let driver = self.store.source_driver(self.driver)?;
                for ((binding, source), location) in
                    bindings.iter().zip(&prepared.sources).zip(locations)
                {
                    driver.verify_restored_materialization_location(
                        location,
                        &source.prepared,
                        &binding.binding,
                    )?;
                }
                let layout = ManagedGitLayout::check(snapshot, locations)?;
                for (binding, source) in bindings.iter().zip(&prepared.sources) {
                    driver.release_restored_materialization(&source.prepared, &binding.binding)?;
                }
                layout.require_released()?;
                Ok(fence)
            })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use aw_agent_core::{
        CheckpointPolicy, SourceAccess, SourceDriverRegistry, SourceName, SourceSetLimit,
    };
    use aw_store::{AgentAdmission, AgentHistoryForkAccess, CheckpointCaptureSelection};
    use rusqlite::Connection;
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, MetadataExt},
        path::{Path, PathBuf},
        process::Command,
        sync::{RwLock, RwLockReadGuard},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn private_dir(path: &Path) {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }

    fn git(executable: &Path, root: &Path, args: &[&str]) -> Vec<u8> {
        let output = Command::new(executable)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn layout(root: &Path) -> SessionBinding {
        SessionBinding {
            working_directory: root.to_owned(),
            tool_root: root.to_owned(),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn identity(path: &Path) -> (u64, u64) {
        let metadata = fs::metadata(path).unwrap();
        (metadata.dev(), metadata.ino())
    }

    fn count_agents(database: &Path) -> u64 {
        Connection::open(database)
            .unwrap()
            .query_row("SELECT count(*) FROM agent_state_current", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    // Fixture authority is a real held read guard. All writers to this
    // isolated fixture are under the test's ownership, not a production
    // claim that a mutex excludes arbitrary filesystem writers.
    struct Fence<'a, L = GitRestoreLocation> {
        locations: Vec<L>,
        _guard: RwLockReadGuard<'a, ()>,
        authority: &'a RwLock<()>,
        database: &'a Path,
        expected_agents: u64,
        expected_retained: Option<u64>,
    }

    impl GitRestoreFence for Fence<'_> {
        fn locations(&self) -> &[GitRestoreLocation] {
            &self.locations
        }
    }

    impl GitSourceFence for Fence<'_, aw_git::source_driver::GitSourceLocation> {
        fn locations(&self) -> &[aw_git::source_driver::GitSourceLocation] {
            &self.locations
        }
    }

    impl<L> Drop for Fence<'_, L> {
        fn drop(&mut self) {
            assert!(self.authority.try_write().is_err());
            assert_eq!(count_agents(self.database), self.expected_agents);
            if let Some(expected) = self.expected_retained {
                let count: u64 = Connection::open(self.database)
                    .unwrap()
                    .query_row("SELECT count(*) FROM checkpoint_sources", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, expected);
            }
        }
    }

    #[test]
    fn git_restore_composes_prepared_custody_fenced_publication_and_exact_recovery() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = Temp(std::env::temp_dir().join(format!(
            "aw-host-git-restore-{}-{nonce}",
            std::process::id()
        )));
        private_dir(&temp.0);
        let root = temp.0.canonicalize().unwrap();
        let source_root = root.join("sources");
        private_dir(&source_root);
        let executable = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let registry = SourceDriverRegistry::new();
        let driver = GitSourceDriver::new(&executable)
            .unwrap()
            .register(&registry)
            .unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let state = root.join("state");
        let database = state.join("sessions.sqlite3");
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let git_driver = store.source_driver(&driver).unwrap();
        let actor = PrincipalId::new("internal:fixture:host-restore").unwrap();
        let (session, branch) = store
            .create(&actor, "initial", layout(&root.join("initial")))
            .unwrap();
        let reference: String = Connection::open(&database).unwrap().query_row(
            "SELECT '/d:' || domain_handle || '/i:' || identity_handle || '/p:' || project_handle || '/a:' || agent_handle FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
            [&session.id, &branch.id], |row| row.get(0),
        ).unwrap();
        let history = store
            .checkpoint_create_history_only(&actor, &reference, None, "fixture:history")
            .unwrap();
        let mut bindings = Vec::new();
        let mut original_heads = Vec::new();
        for name in ["code", "data"] {
            let repo = source_root.join(name);
            private_dir(&repo);
            git(&executable, &repo, &["init", "--initial-branch=main"]);
            git(&executable, &repo, &["config", "user.name", "Fixture"]);
            git(
                &executable,
                &repo,
                &["config", "user.email", "fixture@example.invalid"],
            );
            fs::write(repo.join("tracked"), b"base").unwrap();
            git(&executable, &repo, &["add", "tracked"]);
            git(
                &executable,
                &repo,
                &["commit", "--no-gpg-sign", "-m", "fixture"],
            );
            original_heads.push(git(&executable, &repo, &["rev-parse", "HEAD"]));
            fs::write(repo.join("tracked"), name.as_bytes()).unwrap();
            fs::write(repo.join("untracked"), b"dirty").unwrap();
            bindings.push(
                git_driver
                    .binding(
                        &repo,
                        SourceName::new(name).unwrap(),
                        SourceAccess::ReadWrite,
                        CheckpointPolicy::Required,
                    )
                    .unwrap(),
            );
        }
        let request = CheckpointRestoreRequest {
            checkpoint: history.id,
            project: None,
            title: None,
            access: AgentHistoryForkAccess::Private,
            task: None,
        };
        let history_request = request.clone();
        let empty_auto = store
            .checkpoint_create_empty_auto(&actor, &reference, None, "fixture:empty-auto")
            .unwrap();
        // Use the ordinary lower owners to create and capture the source
        // Agent. No synthetic binding or receipt insertion in the fixture.
        let source = store
            .checkpoint_restore_baseline(
                &actor,
                &request,
                &layout(&source_root),
                &bindings,
                "fixture:baseline",
                "local",
            )
            .unwrap();
        let authority = RwLock::new(());
        let capture_locations = || {
            ["code", "data"]
                .into_iter()
                .map(|name| {
                    let repo = source_root.join(name);
                    aw_git::source_driver::GitSourceLocation {
                        name: SourceName::new(name).unwrap(),
                        repository_identity: identity(&repo),
                        repository: repo,
                    }
                })
                .collect::<Vec<_>>()
        };
        let capture_fence = |locations, expected_agents, expected_retained| Fence {
            locations,
            _guard: authority.read().unwrap(),
            authority: &authority,
            database: &database,
            expected_agents,
            expected_retained: Some(expected_retained),
        };
        let capture_host = GitCheckpointCaptureHost::new(&store, &driver);
        let mut incomplete = capture_locations();
        incomplete.pop();
        assert!(capture_host
            .capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture",
                |_| Ok(capture_fence(incomplete, 2, 0))
            )
            .is_err());
        assert!(store
            .checkpoint_replay_capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture"
            )
            .unwrap()
            .is_none());

        let fail_capture = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER fixture_fail_capture BEFORE INSERT ON checkpoint_sources
                 BEGIN SELECT RAISE(ABORT,'fixture capture publication failure'); END;",
                )
                .unwrap()
        };
        let clear_capture_failure = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch("DROP TRIGGER fixture_fail_capture;")
                .unwrap()
        };
        fail_capture();
        assert!(capture_host
            .capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture",
                |_| Ok(capture_fence(capture_locations(), 2, 0))
            )
            .is_err());
        for name in ["code", "data"] {
            assert!(!git(
                &executable,
                &source_root.join(name),
                &["for-each-ref", "refs/aw/checkpoints/"]
            )
            .is_empty());
        }
        assert!(capture_host
            .capture::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture",
                |_| panic!("unresolved capture authorized again")
            )
            .is_err());
        drop(store);
        clear_capture_failure();
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let capture_host = GitCheckpointCaptureHost::new(&store, &driver);
        let mut wrong = capture_locations();
        wrong[0].repository = wrong[1].repository.clone();
        wrong[0].repository_identity = wrong[1].repository_identity;
        assert!(capture_host
            .resume(&actor, "fixture:capture", |_, _| Ok(capture_fence(
                wrong, 2, 0
            )))
            .is_err());
        let checkpoint = capture_host
            .resume(&actor, "fixture:capture", |_, _| {
                Ok(capture_fence(capture_locations(), 2, 2))
            })
            .unwrap();
        assert_eq!(
            capture_host
                .capture::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                    &actor,
                    &source.agent.canonical_ref,
                    CheckpointCaptureSelection::Auto,
                    None,
                    "fixture:capture",
                    |_| panic!("Applied capture authorized again")
                )
                .unwrap(),
            checkpoint
        );

        // An independent abandoned capture must not release the published refs.
        let before = git(
            &executable,
            &source_root.join("code"),
            &["for-each-ref", "refs/aw/checkpoints/"],
        );
        fail_capture();
        assert!(capture_host
            .capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:abandon-capture",
                |_| Ok(capture_fence(capture_locations(), 2, 2))
            )
            .is_err());
        clear_capture_failure();
        capture_host
            .abandon(&actor, "fixture:abandon-capture", |_, _| {
                Ok(capture_fence(capture_locations(), 2, 2))
            })
            .unwrap();
        assert_eq!(
            git(
                &executable,
                &source_root.join("code"),
                &["for-each-ref", "refs/aw/checkpoints/"]
            ),
            before
        );
        capture_host
            .abandon::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                &actor,
                "fixture:abandon-capture",
                |_, _| panic!("terminal capture cleanup authorized again"),
            )
            .unwrap();
        let request = CheckpointRestoreRequest {
            checkpoint: checkpoint.id,
            task: Some("restore fixture".into()),
            ..request
        };
        let locations = |destination: &Path| {
            ["code", "data"]
                .into_iter()
                .map(|name| {
                    let repo = source_root.join(name);
                    GitRestoreLocation {
                        name: SourceName::new(name).unwrap(),
                        repository_identity: identity(&repo),
                        repository: repo,
                        target: destination.join(name),
                        parent_identity: identity(destination),
                    }
                })
                .collect::<Vec<_>>()
        };
        let fence = |locations, expected_agents| Fence {
            locations,
            _guard: authority.read().unwrap(),
            authority: &authority,
            database: &database,
            expected_agents,
            expected_retained: None,
        };
        let destination = root.join("restored");
        private_dir(&destination);
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let mut incomplete = locations(&destination);
        incomplete.pop();
        // Fixture policy selects these independently retained versions via
        // the real authorized restore observation, never fabricated anchors.
        let mut baseline = None;
        assert!(host
            .restore(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || Ok(layout(&destination)),
                |_, set| {
                    baseline = Some(SourceSet {
                        coverage: aw_agent_core::SourceCoverage::Selected,
                        anchors: set.anchors.clone(),
                    });
                    Ok(fence(incomplete, 2))
                }
            )
            .is_err());
        let baseline = baseline.expect("authorized retained source observation");
        assert!(store
            .checkpoint_replay_restore(&actor, &request, "fixture:restore")
            .unwrap()
            .is_none());
        assert!(!destination.join("code").exists());

        // Abort the real final Store transaction after both effects.
        let fail_publication = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER fixture_fail_restore BEFORE INSERT ON agent_state_current
             BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END;",
                )
                .unwrap()
        };
        let clear_failure = || {
            Connection::open(&database)
                .unwrap()
                .execute_batch("DROP TRIGGER fixture_fail_restore;")
                .unwrap()
        };
        fail_publication();
        assert!(host
            .restore(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || Ok(layout(&destination)),
                |_, _| Ok(fence(locations(&destination), 2))
            )
            .is_err());
        for name in ["code", "data"] {
            assert_eq!(
                fs::read(destination.join(name).join("tracked")).unwrap(),
                name.as_bytes()
            );
            assert_eq!(
                fs::read(destination.join(name).join("untracked")).unwrap(),
                b"dirty"
            );
        }
        assert!(host
            .restore::<Fence<'_>>(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || panic!("unresolved replay selected layout"),
                |_, _| panic!("unresolved replay authorized")
            )
            .is_err());
        drop(store);
        clear_failure();
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let other = root.join("wrong");
        private_dir(&other);
        assert!(host
            .resume(
                &actor,
                "fixture:restore",
                &layout(&destination),
                |_, _, _| Ok(fence(locations(&other), 2))
            )
            .is_err());
        assert!(!other.join("code").exists());
        let restored = host
            .resume(
                &actor,
                "fixture:restore",
                &layout(&destination),
                |_, _, _| Ok(fence(locations(&destination), 3)),
            )
            .unwrap();
        assert_eq!(restored.agent.admission, AgentAdmission::Held);
        assert_eq!(
            host.restore::<Fence<'_>>(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || panic!("applied replay selected layout"),
                |_, _| panic!("applied replay authorized")
            )
            .unwrap(),
            restored
        );
        assert!(host
            .abandon::<Fence<'_>>(&actor, "fixture:restore", |_, _| panic!(
                "applied result cleanup authorized"
            ))
            .is_err());

        let abandoned = root.join("abandoned");
        private_dir(&abandoned);
        fail_publication();
        assert!(host
            .restore(
                &actor,
                &request,
                "fixture:abandon",
                "local",
                || Ok(layout(&abandoned)),
                |_, _| Ok(fence(locations(&abandoned), 3))
            )
            .is_err());
        clear_failure();
        assert!(abandoned.join("data").exists());
        host.abandon(&actor, "fixture:abandon", |_, _| {
            Ok(fence(locations(&abandoned), 3))
        })
        .unwrap();
        assert!(!abandoned.join("code").exists());
        assert!(!abandoned.join("data").exists());
        host.abandon::<Fence<'_>>(&actor, "fixture:abandon", |_, _| {
            panic!("terminal abandonment authorized again")
        })
        .unwrap();

        // A fresh baseline is independent of the checkpoint's historical
        // sources. Required current baseline versions are valid for None,
        // not empty Auto; captured checkpoints may never substitute them.
        let baseline_root = root.join("versioned-baseline");
        private_dir(&baseline_root);
        let auto_request = CheckpointRestoreRequest {
            checkpoint: empty_auto.id,
            ..history_request.clone()
        };
        for invalid in [&request, &auto_request] {
            assert!(host
                .restore_versioned_baseline::<Fence<'_>>(
                    &actor,
                    invalid,
                    "fixture:invalid-baseline",
                    "local",
                    || Ok((layout(&baseline_root), baseline.clone())),
                    |_, _| panic!("invalid baseline reached driver authorization")
                )
                .is_err());
        }
        assert!(!baseline_root.join("code").exists());

        let baseline_request = CheckpointRestoreRequest {
            task: Some("baseline initial task".into()),
            ..history_request
        };
        fail_publication();
        assert!(host
            .restore_versioned_baseline(
                &actor,
                &baseline_request,
                "fixture:versioned-baseline",
                "local",
                || Ok((layout(&baseline_root), baseline.clone())),
                |_, _| Ok(fence(locations(&baseline_root), 3)),
            )
            .is_err());
        assert_eq!(
            fs::read(baseline_root.join("code/tracked")).unwrap(),
            b"code"
        );
        assert!(store
            .checkpoint_delete(&actor, &baseline_request.checkpoint)
            .is_err());
        assert!(host
            .restore_versioned_baseline::<Fence<'_>>(
                &actor,
                &baseline_request,
                "fixture:versioned-baseline",
                "local",
                || panic!("unresolved baseline selected another version"),
                |_, _| panic!("unresolved baseline authorized again")
            )
            .is_err());
        drop(store);
        clear_failure();
        let missing = SessionStore::open(&state).unwrap();
        assert!(GitCheckpointRestoreHost::new(&missing, &driver)
            .resume::<Fence<'_>>(
                &actor,
                "fixture:versioned-baseline",
                &layout(&baseline_root),
                |_, _, _| panic!("missing driver authorized baseline recovery")
            )
            .is_err());
        drop(missing);
        let store = SessionStore::open_with_sources(&state, registry.clone(), limit).unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let versioned = host
            .resume(
                &actor,
                "fixture:versioned-baseline",
                &layout(&baseline_root),
                |_, sources, _| {
                    assert_eq!(sources, &baseline);
                    Ok(fence(locations(&baseline_root), 4))
                },
            )
            .unwrap();
        assert_eq!(
            versioned.source_coverage,
            aw_agent_core::SourceCoverage::None
        );
        assert_eq!(versioned.agent.admission, AgentAdmission::Held);
        assert!(store
            .checkpoint_get_metadata(&actor, &baseline_request.checkpoint)
            .unwrap()
            .sources
            .is_empty());
        assert_eq!(
            host.restore_versioned_baseline::<Fence<'_>>(
                &actor,
                &baseline_request,
                "fixture:versioned-baseline",
                "local",
                || panic!("Applied baseline selected new versions"),
                |_, _| panic!("Applied baseline authorized again")
            )
            .unwrap(),
            versioned
        );

        // Empty Auto accepts only independently selected ephemeral versions.
        // Reuse the ordinary Prepared cleanup, including task-free receipts.
        let ephemeral_root = root.join("ephemeral-baseline");
        private_dir(&ephemeral_root);
        let mut ephemeral = baseline.clone();
        for anchor in &mut ephemeral.anchors {
            anchor.checkpoint_policy = CheckpointPolicy::Ephemeral;
        }
        fail_publication();
        assert!(host
            .restore_versioned_baseline(
                &actor,
                &auto_request,
                "fixture:ephemeral-baseline",
                "local",
                || Ok((layout(&ephemeral_root), ephemeral)),
                |_, _| Ok(fence(locations(&ephemeral_root), 4))
            )
            .is_err());
        clear_failure();
        assert!(ephemeral_root.join("data/tracked").is_file());
        host.abandon(&actor, "fixture:ephemeral-baseline", |_, _| {
            Ok(fence(locations(&ephemeral_root), 4))
        })
        .unwrap();
        assert!(fs::read_dir(&ephemeral_root).unwrap().next().is_none());
        assert!(store
            .checkpoint_get_metadata(&actor, &auto_request.checkpoint)
            .unwrap()
            .sources
            .is_empty());

        // Retire the published layout through its original Agent, queue and
        // Sandbox owners. The checkpoint need not retain its anchor rows.
        let sandbox = restored.agent.sandbox_ref.as_deref().unwrap();
        assert!(host
            .reconcile_closed_sandbox::<Fence<'_>>(&actor, sandbox, |_, _, _| {
                panic!("open Sandbox cleanup authorized")
            })
            .is_err());
        let agent = store
            .agent_execution_binding(&actor, &restored.agent.canonical_ref)
            .unwrap();
        let queue = store.actor_queue_list(&actor, agent.session_id()).unwrap();
        assert_eq!(queue.len(), 1);
        store
            .actor_queue_delete(
                &actor,
                agent.session_id(),
                &queue[0].request.request_id,
                queue[0].request.writer_epoch,
            )
            .unwrap();
        store
            .agent_request_close(&actor, &restored.agent.canonical_ref, None)
            .unwrap();
        let closed_agent = store
            .agent_finish_close(&actor, &restored.agent.canonical_ref)
            .unwrap();
        let closed = store.sandbox_close(&actor, sandbox, None).unwrap();
        store
            .checkpoint_delete(&actor, &request.checkpoint)
            .unwrap();
        let capture_host = GitCheckpointCaptureHost::new(&store, &driver);
        // Physical retention release succeeds, but the Store transaction
        // fails. Exact original anchors remain in Unknown custody; replay
        // reconciles already-absent refs rather than recapturing anything.
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fixture_fail_retirement BEFORE DELETE ON checkpoint_sources
             BEGIN SELECT RAISE(ABORT,'fixture retirement publication failure'); END;",
            )
            .unwrap();
        assert!(capture_host
            .reconcile_retired(&actor, &request.checkpoint, |_| {
                Ok(capture_fence(capture_locations(), 4, 2))
            })
            .is_err());
        for name in ["code", "data"] {
            assert!(git(
                &executable,
                &source_root.join(name),
                &["for-each-ref", "refs/aw/checkpoints/"]
            )
            .is_empty());
        }
        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail_retirement;")
            .unwrap();
        capture_host
            .reconcile_retired(&actor, &request.checkpoint, |_| {
                Ok(capture_fence(capture_locations(), 4, 0))
            })
            .unwrap();
        capture_host
            .reconcile_retired::<Fence<'_, aw_git::source_driver::GitSourceLocation>>(
                &actor,
                &request.checkpoint,
                |_| panic!("released retention authorized again"),
            )
            .unwrap();
        assert!(host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&other), 4))
            })
            .is_err());
        let foreign = destination.join("unrelated");
        fs::write(&foreign, b"keep").unwrap();
        assert!(host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&destination), 4))
            })
            .is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert!(destination.join("code").exists() && destination.join("data").exists());
        fs::remove_file(&foreign).unwrap();

        // A real partial release leaves the whole original layout reserved.
        // Later user work is not discarded even when an earlier checkout
        // was removed successfully.
        let modified = destination.join("data/tracked");
        fs::write(&modified, b"user work").unwrap();
        assert!(host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&destination), 4))
            })
            .is_err());
        assert!(!destination.join("code").exists());
        assert_eq!(fs::read(&modified).unwrap(), b"user work");
        assert_eq!(store.sandbox_get(&actor, sandbox).unwrap(), closed);
        assert!(store
            .create(&actor, "retained layout", layout(&destination))
            .is_err());
        drop(store);

        // The original Applied descriptors and closed binding set survive
        // restart, retirement and a lower new-admission limit.
        let store =
            SessionStore::open_with_sources(&state, registry, SourceSetLimit::new(1).unwrap())
                .unwrap();
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        fs::write(&modified, b"data").unwrap();
        let released = host
            .reconcile_closed_sandbox(&actor, sandbox, |_, _, _| {
                Ok(fence(locations(&destination), 4))
            })
            .unwrap();
        assert!(released.sources.is_empty());
        assert_eq!(released.rev, closed.rev + 1);
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
        assert_eq!(
            host.reconcile_closed_sandbox::<Fence<'_>>(&actor, sandbox, |_, _, _| {
                panic!("released Sandbox cleanup authorized again")
            })
            .unwrap(),
            released
        );
        assert_eq!(
            store
                .agent_get(&actor, &restored.agent.canonical_ref)
                .unwrap(),
            closed_agent
        );
        assert!(store
            .activate(&actor, agent.session_id(), agent.branch_id())
            .is_err());
        assert!(baseline_root.join("code/tracked").is_file());
        assert!(baseline_root.join("data/tracked").is_file());
        assert!(authority.try_write().is_ok());
        for (name, head) in ["code", "data"].into_iter().zip(original_heads) {
            let repo = source_root.join(name);
            assert_eq!(git(&executable, &repo, &["rev-parse", "HEAD"]), head);
            assert_eq!(fs::read(repo.join("tracked")).unwrap(), name.as_bytes());
            assert!(git(&executable, &repo, &["diff", "--cached", "--name-only"]).is_empty());
        }
    }
}
