//! Concrete captured-source Git restore. Store owns Prepared custody,
//! recovery, history and final publication; the host owns physical authority.
//! Blocking trusted-host operations, not an Actor/model/RPC admission route.
use aw_agent_core::{
    Error, ExternalEvidence, NamedPreparedSource, PreparedSourceSet, PrincipalId,
    RecoveryInspection, Result, SourceBinding, SourceDriverHandle, SourceOperationKind,
    SourcePublicationFence, SourcePublicationProof, SourceSet,
};
use aw_git::source_driver::{GitRestoreLocation, GitSourceAnchor, GitSourceDriver};
use aw_store::{
    CheckpointRestoreRequest, CheckpointRestoreResult, InspectedCheckpointRestore, SessionBinding,
    SessionStore,
};

/// Live, independently authorized, whole-layout physical/provider fence.
///
/// The trusted authority owner must guarantee current actor/source disclosure,
/// repository administration and retention, destination isolation (including
/// aliases and other Stores), and the permitted restore or cleanup effect until
/// this guard drops. Locations are immutable, in canonical anchor-name order.
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

impl<F: GitRestoreFence> SourcePublicationFence for Publication<'_, F> {
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
    anchors
        .anchors
        .iter()
        .map(GitSourceAnchor::try_from)
        .collect()
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
    /// Empty/baseline and non-Git SourceSets require their own host route.
    pub fn restore<F: GitRestoreFence>(
        &self,
        actor: &PrincipalId,
        request: &CheckpointRestoreRequest,
        request_id: &str,
        execution_site: &str,
        select_layout: impl FnOnce() -> Result<SessionBinding>,
        authorize: impl FnOnce(&SessionBinding, &SourceSet) -> Result<F>,
    ) -> Result<CheckpointRestoreResult> {
        if let Some(result) = self
            .store
            .checkpoint_replay_restore(actor, request, request_id)?
        {
            return Ok(result);
        }
        let layout = select_layout()?;
        let driver = self.store.source_driver(self.driver)?;
        let mut fence = None;
        let prepared = self.store.checkpoint_prepare_restore(
            actor,
            request,
            &layout,
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
    struct Fence<'a> {
        locations: Vec<GitRestoreLocation>,
        _guard: RwLockReadGuard<'a, ()>,
        authority: &'a RwLock<()>,
        database: &'a Path,
        expected_agents: u64,
    }

    impl GitRestoreFence for Fence<'_> {
        fn locations(&self) -> &[GitRestoreLocation] {
            &self.locations
        }
    }

    impl Drop for Fence<'_> {
        fn drop(&mut self) {
            assert!(self.authority.try_write().is_err());
            assert_eq!(count_agents(self.database), self.expected_agents);
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
        let capture = store
            .checkpoint_prepare_capture(
                &actor,
                &source.agent.canonical_ref,
                CheckpointCaptureSelection::Auto,
                None,
                "fixture:capture",
                |id, bindings| {
                    Ok(PreparedSourceSet {
                        prepare_id: id.to_owned(),
                        operation: SourceOperationKind::Capture,
                        sources: bindings
                            .iter()
                            .map(|binding| {
                                Ok(NamedPreparedSource {
                                    name: binding.binding.name.clone(),
                                    prepared: git_driver.plan_capture(&binding.binding, id, id)?,
                                })
                            })
                            .collect::<Result<_>>()?,
                    })
                },
            )
            .unwrap();
        capture.revalidate().unwrap();
        for source in &capture.sources().sources {
            git_driver.execute_capture(&source.prepared).unwrap();
        }
        let captured = capture
            .inspect(|_, _, prepared| {
                prepared
                    .sources
                    .iter()
                    .map(|source| {
                        let RecoveryInspection::Completed { result, evidence } =
                            git_driver.inspect_capture(&source.prepared)?
                        else {
                            return Err(Error::Conflict("fixture capture incomplete"));
                        };
                        RecoveryInspection::completed((&result).try_into()?, evidence)
                    })
                    .collect()
            })
            .unwrap();
        let checkpoint = captured
            .publish::<std::convert::Infallible>(|_, _, prepared, sources, evidence| {
                prepared
                    .sources
                    .iter()
                    .zip(&sources.anchors)
                    .zip(evidence)
                    .map(|((source, anchor), evidence)| {
                        git_driver.revalidate_capture(
                            &source.prepared,
                            &GitSourceAnchor::try_from(anchor)?,
                        )?;
                        Ok(SourcePublicationProof::Immutable(evidence.clone()))
                    })
                    .collect()
            })
            .unwrap();
        let request = CheckpointRestoreRequest {
            checkpoint: checkpoint.id,
            task: Some("restore fixture".into()),
            ..request
        };
        let authority = RwLock::new(());
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
        };
        let destination = root.join("restored");
        private_dir(&destination);
        let host = GitCheckpointRestoreHost::new(&store, &driver);
        let mut incomplete = locations(&destination);
        incomplete.pop();
        assert!(host
            .restore(
                &actor,
                &request,
                "fixture:restore",
                "local",
                || Ok(layout(&destination)),
                |_, _| Ok(fence(incomplete, 2))
            )
            .is_err());
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
        let store = SessionStore::open_with_sources(&state, registry, limit).unwrap();
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
        assert!(authority.try_write().is_ok());
        for (name, head) in ["code", "data"].into_iter().zip(original_heads) {
            let repo = source_root.join(name);
            assert_eq!(git(&executable, &repo, &["rev-parse", "HEAD"]), head);
            assert_eq!(fs::read(repo.join("tracked")).unwrap(), name.as_bytes());
            assert!(git(&executable, &repo, &["diff", "--cached", "--name-only"]).is_empty());
        }
    }
}
