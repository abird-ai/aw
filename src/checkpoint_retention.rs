//! Offline trusted host composition for exact retired Git retention only.
//! No Agent activation, ambient repository discovery or client RPC authority.
use crate::git_maintenance::{
    lock_private_approval, overlaps, pin_private_checkout, require_existing_store,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{validate_source_set_hard_limit, SourceSet};
use aw_git::source_driver::{GitSourceAnchor, GitSourceLocation};
use aw_host::source_checkpoint::{reconcile_retired_git, GitSourceFence};
use aw_platform::DirectoryIdentity;
use aw_store::SessionStore;
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_APPROVAL_BYTES: usize = 1024 * 1024;

// An independently supplied administrative decision, not a stored SandboxGrant
// or an authorization inferred from retained source metadata.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    store_directory: PathBuf,
    store_id: String,
    store_generation: u64,
    manager: PrincipalId,
    checkpoint: String,
    repositories: Vec<GitSourceLocation>,
}

struct Approval {
    config: Config,
    directory: PathBuf,
    // Cooperating host approval writers must acquire this same exclusive lock
    // before changing/withdrawing the decision. Never replace its inode while
    // live. The operator independently excludes external physical writers and
    // other retention users. This lock does not provide filesystem isolation.
    _lease: File,
}

impl Approval {
    fn open(path: &Path, manager: &PrincipalId) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or(Error::Denied("retention approval directory required"))?;
        let file = lock_private_approval(path)?;
        let mut bytes = Vec::new();
        (&file)
            .take(MAX_APPROVAL_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_APPROVAL_BYTES {
            return Err(Error::Limit("retention approval exceeds 1 MiB"));
        }
        let mut config: Config = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Config("invalid Git retention approval".into()))?;
        if config.version != 1
            || &config.manager != manager
            || config.store_generation == 0
            || config.checkpoint.is_empty()
            || config.checkpoint.len() > 128
            || !config
                .checkpoint
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
        {
            return Err(Error::Denied(
                "retention approval differs from host manager or checkpoint",
            ));
        }
        validate_source_set_hard_limit(config.repositories.len())?;
        for repository in &config.repositories {
            repository.name.validate()?;
            if !repository.repository.is_absolute() || repository.repository.parent().is_none() {
                return Err(Error::Denied(
                    "retention approval requires exact checkout roots",
                ));
            }
        }
        config
            .repositories
            .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        if config
            .repositories
            .windows(2)
            .any(|pair| pair[0].name == pair[1].name)
        {
            return Err(Error::Denied("duplicate retention approval source name"));
        }
        if overlaps(parent, &config.store_directory) {
            return Err(Error::Denied(
                "retention approval must be outside Store state",
            ));
        }
        require_existing_store(&config.store_directory)?;
        Ok(Self {
            config,
            directory: parent.to_owned(),
            _lease: file,
        })
    }

    fn reconcile(&self, store: &SessionStore) -> Result<()> {
        let c = &self.config;
        let process = store.local_process_info()?;
        if store.directory() != Some(c.store_directory.as_path())
            || store.store_id()? != c.store_id
            || process.store_generation != c.store_generation
            || process.process_kind != "owner_lock"
            || process.state != "active"
        {
            return Err(Error::Denied(
                "retention approval differs from current Store",
            ));
        }
        reconcile_retired_git(store, &c.manager, &c.checkpoint, |set| self.fence(set))
    }

    fn fence(&self, set: &SourceSet) -> Result<Fence<'_>> {
        let locations = &self.config.repositories;
        if locations.len() != set.anchors.len() || locations.is_empty() {
            return Err(Error::Denied(
                "retention approval must cover the exact retired set",
            ));
        }
        let mut pins = Vec::with_capacity(locations.len() * 3);
        for (location, anchor) in locations.iter().zip(&set.anchors) {
            let anchor = GitSourceAnchor::try_from(anchor)?;
            if anchor.name != location.name || anchor.checkpoint_id != self.config.checkpoint {
                return Err(Error::Denied(
                    "retention approval names another checkpoint or source",
                ));
            }
            pins.extend(pin_private_checkout(
                location,
                &[&self.config.store_directory, &self.directory],
            )?);
        }
        Ok(Fence {
            _approval: self,
            locations,
            _pins: pins,
        })
    }
}

struct Fence<'a> {
    _approval: &'a Approval,
    locations: &'a [GitSourceLocation],
    _pins: Vec<DirectoryIdentity>,
}

impl GitSourceFence for Fence<'_> {
    fn locations(&self) -> &[GitSourceLocation] {
        self.locations
    }
}

pub(crate) fn run(path: &Path) -> Result<u8> {
    let manager = PrincipalId::new(aw_platform::local_principal()?)?;
    let approval = Approval::open(path, &manager)?;
    let store = aw::sessions::open_sqlite_owner(&approval.config.store_directory)?;
    approval.reconcile(&store)?;
    super::print_json(&serde_json::json!({"status":"settled"}))?;
    Ok(0)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use aw_agent_core::{
        CheckpointPolicy, NamedPreparedSource, PreparedSourceSet, SourceAccess,
        SourceDriverRegistry, SourceName, SourceOperationKind,
    };
    use aw_git::source_driver::GitSourceDriver;
    use aw_store::{
        AgentHistoryForkAccess, CheckpointCaptureRequest, CheckpointCaptureSelection,
        CheckpointRestoreRequest, SessionBinding, SourceMutationOutcome, SourceMutationSet,
    };
    use aw_store_api::SessionBackend;
    use serde_json::json;
    use std::{fs, io::Write, process::Command};

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn git(executable: &Path, root: &Path, args: &[&str]) -> String {
        let out = Command::new(executable)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "fixture Git failed: {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
    }

    #[test]
    fn offline_configured_capture_and_retention_reconcile_exact_git_refs_without_activation() {
        let temp = Temp(crate::test_support::temp_dir("offline-retention"));
        let root = temp.0.canonicalize().unwrap();
        let repo = root.join("repo");
        let approvals = root.join("approvals");
        let initial = root.join("initial");
        for path in [&repo, &approvals, &initial] {
            aw_platform::ensure_private_dir(path).unwrap();
        }
        let state = root.join("state");
        let executable = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        git(&executable, &repo, &["init", "--initial-branch=main"]);
        git(&executable, &repo, &["config", "user.name", "Fixture"]);
        git(
            &executable,
            &repo,
            &["config", "user.email", "fixture@example.invalid"],
        );
        fs::write(repo.join("tracked"), b"original").unwrap();
        git(&executable, &repo, &["add", "tracked"]);
        git(
            &executable,
            &repo,
            &["commit", "--no-gpg-sign", "-m", "fixture"],
        );
        let head = git(&executable, &repo, &["rev-parse", "HEAD"]);
        let index = fs::read(repo.join(".git/index")).unwrap();
        fs::write(repo.join("tracked"), b"dirty checkpoint").unwrap();
        let registry = SourceDriverRegistry::new();
        let handle = GitSourceDriver::new(&executable)
            .unwrap()
            .register(&registry)
            .unwrap();
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let actor = PrincipalId::new(aw_platform::local_principal().unwrap()).unwrap();
        let layout = |path: &Path| SessionBinding {
            working_directory: path.to_owned(),
            tool_root: path.to_owned(),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (session, branch) = store.create(&actor, "fixture", layout(&initial)).unwrap();
        let info = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let reference = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            info.domain_handle, info.identity_handle, info.project_handle, info.agent_handle
        );
        let history = store
            .checkpoint_create_history_only(&actor, &reference, None, "fixture:history")
            .unwrap();
        let name = SourceName::new("code").unwrap();
        let binding = store
            .source_driver(&handle)
            .unwrap()
            .binding(
                &repo,
                name.clone(),
                SourceAccess::ReadWrite,
                CheckpointPolicy::Required,
            )
            .unwrap();
        let restored = store
            .checkpoint_restore_baseline(
                &actor,
                &CheckpointRestoreRequest {
                    checkpoint: history.id,
                    project: None,
                    title: None,
                    access: AgentHistoryForkAccess::Private,
                    task: None,
                },
                &layout(&repo),
                std::slice::from_ref(&binding),
                "fixture:baseline",
                "local",
            )
            .unwrap();
        let location = GitSourceLocation {
            name,
            repository: repo.clone(),
            repository_identity: DirectoryIdentity::open(&repo).unwrap().identity().unwrap(),
        };
        // This isolated fixture owns all checkout, Git administration and config
        // writers. Exercise the real configured offline owner, not a fake grant.
        let agent_ref = &restored.agent.canonical_ref;
        let sandbox = store
            .sandbox_get(&actor, restored.agent.sandbox_ref.as_deref().unwrap())
            .unwrap();
        let grant_before = store.sandbox_grant_status(&actor, agent_ref).unwrap();
        let agent_before = store.agent_get(&actor, agent_ref).unwrap();
        let request = CheckpointCaptureRequest {
            reference: agent_ref,
            selection: CheckpointCaptureSelection::Auto,
            note: Some("offline fixture"),
        };
        let native_config = json!({
            "version":1,"store_directory":state,
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor,"actor":actor,"agent_ref":agent_ref,
            "sandbox_ref":sandbox.canonical_ref,"sandbox_rev":sandbox.rev,
            "policy_ref":"host:fixture-offline-git","base":repo,
            "directories":[{"name":location.name,"repository":location.repository,
                "repository_identity":location.repository_identity,
                "binding_rev":sandbox.sources[0].binding_rev,"access":"read_write"}]
        });
        let native_path = approvals.join("git-native.json");
        let mut wrong_native = native_config.clone();
        wrong_native["directories"][0]["binding_rev"] = json!(sandbox.sources[0].binding_rev + 1);
        let mut native_file = aw_platform::open_private_new(&native_path).unwrap();
        native_file
            .write_all(&serde_json::to_vec(&wrong_native).unwrap())
            .unwrap();
        drop(native_file);
        let invoke = |store: &SessionStore, root: &Path, id: &str, recover: bool| {
            crate::checkpoint_capture::capture(store, &actor, root, &request, id, recover)
        };
        assert!(invoke(&store, &approvals, "fixture:fresh-capture", false).is_err());
        assert!(git(
            &executable,
            &repo,
            &["for-each-ref", "refs/aw/checkpoints/"]
        )
        .is_empty());
        assert_eq!(fs::read_dir(&approvals).unwrap().count(), 1);
        fs::write(&native_path, serde_json::to_vec(&native_config).unwrap()).unwrap();
        let held_config = lock_private_approval(&native_path).unwrap();
        assert!(invoke(&store, &approvals, "fixture:fresh-capture", false).is_err());
        drop(held_config);
        let fresh = invoke(&store, &approvals, "fixture:fresh-capture", false).unwrap();
        assert_eq!(fresh.sources.len(), 1);
        assert_eq!(
            invoke(
                &store,
                &root.join("absent-approval"),
                "fixture:fresh-capture",
                false
            )
            .unwrap(),
            fresh
        );
        assert_eq!(
            store.sandbox_grant_status(&actor, agent_ref).unwrap(),
            grant_before
        );
        assert_eq!(store.agent_get(&actor, agent_ref).unwrap(), agent_before);

        // Interrupt a real prepared capture after retention creation, before
        // publication, then reopen and recover through the configured path.
        let driver = store.source_driver(&handle).unwrap();
        let prepared = store
            .checkpoint_prepare_capture(
                &actor,
                agent_ref,
                request.selection.clone(),
                request.note,
                "fixture:capture",
                |checkpoint, bindings| {
                    assert_eq!(bindings.len(), 1);
                    Ok(PreparedSourceSet {
                        prepare_id: checkpoint.to_owned(),
                        operation: SourceOperationKind::Capture,
                        sources: vec![NamedPreparedSource {
                            name: location.name.clone(),
                            prepared: driver.plan_capture_approved(
                                &location,
                                &bindings[0].binding,
                                checkpoint,
                                checkpoint,
                            )?,
                        }],
                    })
                },
            )
            .unwrap();
        let interrupted_id = prepared.checkpoint_id().to_owned();
        prepared.revalidate().unwrap();
        driver
            .execute_capture(&prepared.sources().sources[0].prepared)
            .unwrap();
        prepared.mark_unknown().unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        assert!(invoke(&store, &approvals, "fixture:capture", false).is_err());
        let wrong_request = CheckpointCaptureRequest {
            note: Some("different request"),
            ..request.clone()
        };
        assert!(store
            .checkpoint_inspect_capture_exact(
                &actor,
                &wrong_request,
                "fixture:capture",
                |_, _, _| panic!("mismatched receipt reached inspection")
            )
            .is_err());
        assert!(crate::checkpoint_capture::capture(
            &store,
            &actor,
            &approvals,
            &wrong_request,
            "fixture:capture",
            true
        )
        .is_err());
        let capture = invoke(&store, &approvals, "fixture:capture", true).unwrap();
        assert_eq!(capture.id, interrupted_id);
        assert_eq!(
            store.sandbox_grant_status(&actor, agent_ref).unwrap(),
            grant_before
        );
        assert_eq!(store.agent_get(&actor, agent_ref).unwrap(), agent_before);
        let reference = format!("refs/aw/checkpoints/{}/code", capture.id);
        let captured = git(&executable, &repo, &["rev-parse", &reference]);
        assert_ne!(captured, head);
        let config = json!({"version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
            "store_generation":store.local_process_info().unwrap().store_generation,"manager":actor,
            "checkpoint":capture.id,"repositories":[location]});
        let path = approvals.join("retention.json");
        let mut file = aw_platform::open_private_new(&path).unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        drop(file);
        let approval = Approval::open(&path, &actor).unwrap();
        assert!(Approval::open(&path, &actor).is_err());
        assert!(
            approval.reconcile(&store).is_err(),
            "live checkpoint must not release"
        );
        store.checkpoint_delete(&actor, &capture.id).unwrap();
        let before = store
            .agent_get(&actor, &restored.agent.canonical_ref)
            .unwrap();
        drop(approval);
        // Reject a substituted approval before a new cleanup claim.
        let mut wrong = config.clone();
        wrong["repositories"][0]["repository_identity"] = json!([0, 0]);
        fs::write(&path, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(Approval::open(&path, &actor)
            .unwrap()
            .reconcile(&store)
            .is_err());
        assert_eq!(
            git(&executable, &repo, &["rev-parse", &reference]),
            captured
        );
        let mutations = SourceMutationSet::from_bindings(&[binding]).unwrap();
        store
            .acquire_source_mutation_barrier(&mutations, "fixture:preflight-no-hazard")
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let approval = Approval::open(&path, &actor).unwrap();
        // A changed ref is not permission to delete its replacement.
        git(
            &executable,
            &repo,
            &["update-ref", &reference, &head, &captured],
        );
        assert!(approval.reconcile(&store).is_err());
        assert_eq!(git(&executable, &repo, &["rev-parse", &reference]), head);
        // Retired public snapshots deliberately hide retained anchors. Check
        // the actual blocked mutation gate, not presentation metadata.
        assert!(store
            .acquire_source_mutation_barrier(&mutations, "fixture:uncertain-release")
            .is_err());
        drop(approval);
        drop(store);
        // Operator restores the exact original retained ref in the isolated fixture.
        git(
            &executable,
            &repo,
            &["update-ref", &reference, &captured, &head],
        );
        let store = SessionStore::open_with_sources(&state, registry, Default::default()).unwrap();
        Approval::open(&path, &actor)
            .unwrap()
            .reconcile(&store)
            .unwrap();
        assert!(git(&executable, &repo, &["for-each-ref", &reference]).is_empty());
        // Independent live checkpoint retention was never part of this cleanup.
        assert!(!git(
            &executable,
            &repo,
            &[
                "for-each-ref",
                &format!("refs/aw/checkpoints/{}/code", fresh.id)
            ]
        )
        .is_empty());
        store
            .acquire_source_mutation_barrier(&mutations, "fixture:retention-settled")
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();
        assert_eq!(
            store
                .agent_get(&actor, &restored.agent.canonical_ref)
                .unwrap(),
            before
        );
        assert_eq!(git(&executable, &repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(repo.join("tracked")).unwrap(), b"dirty checkpoint");
        drop(store);
        // Terminal replay uses the tombstone even without installed Git or the checkout.
        fs::rename(&repo, root.join("moved-repo")).unwrap();
        let store = SessionStore::open(&state).unwrap();
        assert_eq!(
            invoke(
                &store,
                &root.join("absent-approval"),
                "fixture:capture",
                true
            )
            .unwrap()
            .lifecycle,
            aw_store::CheckpointLifecycle::Retired
        );
        Approval::open(&path, &actor)
            .unwrap()
            .reconcile(&store)
            .unwrap();
    }
}
