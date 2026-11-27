//! Offline trusted host composition for exact retired Git retention only.
//! No Agent activation, ambient repository discovery or client RPC authority.
use crate::git_maintenance::{
    open_private_approval, overlaps, pin_private_checkout, require_existing_store,
    require_store_scope, validate_named_locations,
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::SourceSet;
use aw_git::source_driver::{GitSourceAnchor, GitSourceLocation};
use aw_host::source_checkpoint::{reconcile_retired_git, GitSourceFence};
use aw_platform::DirectoryIdentity;
use aw_store::SessionStore;
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

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
        let (file, mut config): (_, Config) = open_private_approval(path)?;
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
        validate_named_locations(&mut config.repositories)?;
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
        self.reconcile_protected(store, &[])
    }

    fn reconcile_protected(&self, store: &SessionStore, protected: &[&Path]) -> Result<()> {
        let c = &self.config;
        require_store_scope(store, &c.store_directory, &c.store_id, c.store_generation)?;
        reconcile_retired_git(store, &c.manager, &c.checkpoint, |set| {
            self.fence(set, protected)
        })
    }

    fn fence(&self, set: &SourceSet, protected: &[&Path]) -> Result<Fence<'_>> {
        let locations = &self.config.repositories;
        if locations.len() != set.anchors.len() || locations.is_empty() {
            return Err(Error::Denied(
                "retention approval must cover the exact retired set",
            ));
        }
        let mut exclusions = vec![
            self.config.store_directory.as_path(),
            self.directory.as_path(),
        ];
        exclusions.extend_from_slice(protected);
        let mut pins = Vec::with_capacity(locations.len() * 3);
        for (location, anchor) in locations.iter().zip(&set.anchors) {
            let anchor = GitSourceAnchor::try_from(anchor)?;
            if anchor.name != location.name || anchor.checkpoint_id != self.config.checkpoint {
                return Err(Error::Denied(
                    "retention approval names another checkpoint or source",
                ));
            }
            pins.extend(pin_private_checkout(location, &exclusions)?);
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

/// Explicit local host capability; exact target and Project checked before
/// any provider work. An existing retired tombstone does not grant cleanup.
pub(crate) fn reconcile_configured(
    store: &SessionStore,
    manager: &PrincipalId,
    path: &Path,
    project: &str,
    checkpoint: &str,
    protected: &[&Path],
) -> Result<()> {
    let approval = Approval::open(path, manager)?;
    let c = &approval.config;
    if c.checkpoint != checkpoint {
        return Err(Error::Denied("retention approval names another checkpoint"));
    }
    require_store_scope(store, &c.store_directory, &c.store_id, c.store_generation)?;
    let snapshot = store.checkpoint_get_metadata(manager, checkpoint)?;
    if snapshot.source_agent_ref.rsplit_once("/a:").map(|(p, _)| p) != Some(project) {
        return Err(Error::Denied(
            "cross-Project checkpoint retention unavailable",
        ));
    }
    approval.reconcile_protected(store, protected)
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
    use crate::git_maintenance::lock_private_approval;
    use aw_agent_core::{
        CheckpointPolicy, NamedPreparedSource, PreparedSourceSet, SourceAccess,
        SourceDriverRegistry, SourceName, SourceOperationKind,
    };
    use aw_git::source_driver::{GitRestoreLocation, GitSourceDriver};
    use aw_store::{
        AgentAdmission, AgentHistoryForkAccess, CheckpointCaptureRequest,
        CheckpointCaptureSelection, CheckpointRestoreRequest, SessionBinding,
        SourceMutationOutcome, SourceMutationSet,
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
    fn offline_git_checkpoint_lifecycle_uses_exact_approval_without_activation() {
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
        assert!(invoke(
            &store,
            &approvals,
            "rpc:stdio:checkpoint:fixture-fresh-capture",
            false
        )
        .is_err());
        assert!(git(
            &executable,
            &repo,
            &["for-each-ref", "refs/aw/checkpoints/"]
        )
        .is_empty());
        assert_eq!(fs::read_dir(&approvals).unwrap().count(), 1);
        fs::write(&native_path, serde_json::to_vec(&native_config).unwrap()).unwrap();
        let held_config = lock_private_approval(&native_path).unwrap();
        assert!(invoke(
            &store,
            &approvals,
            "rpc:stdio:checkpoint:fixture-fresh-capture",
            false
        )
        .is_err());
        drop(held_config);
        // The successful fresh invocation goes through the actual RPC
        // constructor port. The interrupted cases below still test the
        // independently approved original offline recovery owner.
        use aw::headless::{CheckpointCreatePort, CheckpointRestorePort};
        let construction_state = |store: &SessionStore| {
            std::sync::Arc::new(crate::StdioAgentStatePort {
                store: store.clone(),
                caller: actor.clone(),
                current_project_ref: agent_before.project_ref.clone(),
                current_agent_ref: agent_ref.clone(),
                source_metadata: None,
            })
        };
        let capture_port =
            |store: &SessionStore, root: PathBuf| crate::source_construction_rpc::CapturePort {
                state: construction_state(store),
                approval_root: root,
                history_enabled: false,
            };
        let capture_params = json!({"ref":agent_ref,"sources":"auto","note":"offline fixture"});
        assert!(capture_port(&store, approvals.clone())
            .create(
                "foreign",
                json!({"ref":"/d:foreign/i:foreign/p:foreign/a:foreign","sources":"auto"})
            )
            .is_err());
        assert!(capture_port(&store, approvals.clone())
            .create("none", json!({"sources":"none"}))
            .is_err());
        let captured = capture_port(&store, approvals.clone())
            .create("fixture-fresh-capture", capture_params.clone())
            .unwrap();
        let fresh = invoke(
            &store,
            &approvals,
            "rpc:stdio:checkpoint:fixture-fresh-capture",
            false,
        )
        .unwrap();
        assert_eq!(captured["id"], fresh.id);
        assert_eq!(captured["sources"].as_array().unwrap().len(), 1);
        assert!(captured["sources"][0].get("source_ref").is_none());
        assert!(captured["sources"][0].get("source").is_none());
        assert_eq!(
            capture_port(&store, root.join("absent-approval"))
                .create("fixture-fresh-capture", capture_params.clone())
                .unwrap(),
            captured
        );
        assert!(capture_port(&store, approvals.clone())
            .create(
                "fixture-fresh-capture",
                json!({"ref":agent_ref,"sources":["code"],"note":"offline fixture"})
            )
            .is_err());
        assert_eq!(fresh.sources.len(), 1);
        assert_eq!(
            invoke(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:checkpoint:fixture-fresh-capture",
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

        // The existing metadata ports disclose only an independently approved
        // exact source identity. No Git probe is part of a metadata read.
        use aw::headless::{CheckpointReadPort, SandboxPort};
        use std::sync::Arc;
        let metadata_path = approvals.join("metadata.json");
        let project = &agent_before.project_ref;
        let fresh_version = git(
            &executable,
            &repo,
            &[
                "rev-parse",
                &format!("refs/aw/checkpoints/{}/code", fresh.id),
            ],
        );
        let binding_identity = json!({"name":binding.name,"driver":binding.driver,"schema":binding.binding_schema,
            "source_ref":binding.source_ref});
        let anchor_identity = json!({"name":binding.name,"driver":binding.driver,"schema":fresh.sources[0].anchor_schema,
            "source_ref":binding.source_ref,"version":fresh_version});
        let metadata_config = json!({"version":1,"store_directory":state,"policy":{
            "store_id":store.store_id().unwrap(),"store_generation":store.local_process_info().unwrap().store_generation,
            "actor":actor,"actor_user_id":store.current_user(&actor).unwrap().user_id,"project":project,
            "sources":[{"kind":"binding","sandbox":sandbox.canonical_ref,"sandbox_rev":sandbox.rev,
                "binding_rev":sandbox.sources[0].binding_rev,"source":binding_identity},
                {"kind":"anchor","checkpoint":fresh.id,"source":anchor_identity}]
        }});
        let mut metadata_file = aw_platform::open_private_new(&metadata_path).unwrap();
        metadata_file
            .write_all(&serde_json::to_vec(&metadata_config).unwrap())
            .unwrap();
        drop(metadata_file);
        let metadata = Arc::new(
            crate::source_metadata::ApprovalFile::open(
                &metadata_path,
                &store,
                &actor,
                project,
                &layout(&initial),
            )
            .unwrap(),
        );
        let sandbox_port = |store: &SessionStore, approval: bool| crate::StdioSandboxPort {
            store: store.clone(),
            caller: actor.clone(),
            current_project_ref: project.clone(),
            source_metadata: approval.then(|| metadata.clone()),
        };
        let metadata_sandbox = |store: &SessionStore, approval: bool, method: &str| {
            sandbox_port(store, approval).apply(method, json!({"ref":sandbox.canonical_ref}))
        };
        let metadata_checkpoint =
            |store: &SessionStore, approval: bool, method: &str, params: serde_json::Value| {
                CheckpointReadPort::apply(
                    &crate::StdioAgentStatePort {
                        store: store.clone(),
                        caller: actor.clone(),
                        current_project_ref: project.clone(),
                        current_agent_ref: agent_ref.clone(),
                        source_metadata: approval.then(|| metadata.clone()),
                    },
                    method,
                    params,
                )
            };
        let get_checkpoint = |store: &SessionStore, approval: bool, id: &str| {
            metadata_checkpoint(store, approval, "checkpoint_get", json!({"id":id}))
        };
        let redacted = metadata_sandbox(&store, false, "sandbox_get").unwrap();
        assert!(redacted["sources"][0].get("source").is_none());
        let visible = metadata_sandbox(&store, true, "sandbox_sources_list").unwrap();
        assert_eq!(visible["sources"][0]["source"], binding_identity);
        assert_eq!(
            metadata_sandbox(&store, true, "sandbox_get").unwrap()["sources"],
            visible["sources"]
        );
        assert!(
            get_checkpoint(&store, false, &fresh.id).unwrap()["sources"][0]
                .get("source")
                .is_none()
        );
        assert_eq!(
            get_checkpoint(&store, true, &fresh.id).unwrap()["sources"][0]["source"],
            anchor_identity
        );
        let page =
            metadata_checkpoint(&store, true, "checkpoint_list", json!({"limit":16})).unwrap();
        assert_eq!(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["id"] == fresh.id)
                .unwrap()["sources"][0]["source"],
            anchor_identity
        );
        for value in [visible, get_checkpoint(&store, true, &fresh.id).unwrap()] {
            let encoded = value.to_string();
            assert!(!encoded.contains(&binding.materialization));
            assert!(!encoded.contains("refs/aw/checkpoints/"));
            assert!(!encoded.contains("layout_digest"));
        }
        let included = || json!(["aw.source_metadata", "fixture.missing"]);
        let view_checkpoint = |store: &SessionStore, approval: bool| {
            metadata_checkpoint(
                store,
                approval,
                "checkpoint_get",
                json!({"id":fresh.id,"include":included()}),
            )
        };
        let view_sandbox = |store: &SessionStore, approval: bool| {
            sandbox_port(store, approval).apply(
                "sandbox_get",
                json!({"ref":sandbox.canonical_ref,"include":included()}),
            )
        };
        let core = |mut value: serde_json::Value| {
            value.as_object_mut().unwrap().remove("extensions");
            value
        };
        let protected_view =
            |value: &serde_json::Value| value["extensions"]["aw.source_metadata"].clone();
        for (value, expected, baseline) in [
            (
                view_sandbox(&store, true).unwrap(),
                binding_identity,
                redacted.clone(),
            ),
            (
                view_checkpoint(&store, true).unwrap(),
                anchor_identity.clone(),
                get_checkpoint(&store, false, &fresh.id).unwrap(),
            ),
        ] {
            assert_eq!(
                protected_view(&value),
                json!({"status":"ok","value":[expected]})
            );
            assert_eq!(
                value["extensions"]["fixture.missing"],
                json!({"status":"error","code":"unknown"})
            );
            assert_eq!(core(value), baseline);
        }
        let mut wrong = metadata_config.clone();
        wrong["policy"]["sources"][0]["binding_rev"] = json!(sandbox.sources[0].binding_rev + 1);
        wrong["policy"]["sources"][1]["source"]["version"] = json!(head);
        fs::write(&metadata_path, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(
            metadata_sandbox(&store, true, "sandbox_get").unwrap()["sources"][0]
                .get("source")
                .is_none()
        );
        assert!(
            get_checkpoint(&store, true, &fresh.id).unwrap()["sources"][0]
                .get("source")
                .is_none()
        );
        let mut wrong = metadata_config.clone();
        wrong["policy"]["actor_user_id"] = json!("another-user");
        fs::write(&metadata_path, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert_eq!(
            get_checkpoint(&store, true, &fresh.id).unwrap(),
            get_checkpoint(&store, false, &fresh.id).unwrap()
        );
        let denied = view_checkpoint(&store, true).unwrap();
        assert_eq!(
            protected_view(&denied),
            json!({"status":"error","code":"denied"})
        );
        assert_eq!(
            core(denied),
            get_checkpoint(&store, false, &fresh.id).unwrap()
        );
        assert_eq!(
            metadata_checkpoint(&store, true, "checkpoint_list", json!({"limit":16})).unwrap(),
            metadata_checkpoint(&store, false, "checkpoint_list", json!({"limit":16})).unwrap()
        );
        fs::write(
            &metadata_path,
            serde_json::to_vec(&metadata_config).unwrap(),
        )
        .unwrap();
        let held = lock_private_approval(&metadata_path).unwrap();
        assert_eq!(
            metadata_sandbox(&store, true, "sandbox_get").unwrap(),
            redacted
        );
        let locked = view_sandbox(&store, true).unwrap();
        assert_eq!(
            protected_view(&locked),
            json!({"status":"error","code":"unavailable"})
        );
        assert_eq!(core(locked), redacted);
        drop(held);
        assert_eq!(
            store.sandbox_grant_status(&actor, agent_ref).unwrap(),
            grant_before
        );
        assert_eq!(store.agent_get(&actor, agent_ref).unwrap(), agent_before);

        // Interrupt a real prepared capture after retention creation, before
        // publication, then reopen and recover through the configured path.
        let driver = store.source_driver(&handle).unwrap();
        let interrupt_capture = |store: &SessionStore, id: &str| {
            let prepared = store
                .checkpoint_prepare_capture(
                    &actor,
                    agent_ref,
                    request.selection.clone(),
                    request.note,
                    id,
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
            let candidate = prepared.checkpoint_id().to_owned();
            prepared.revalidate().unwrap();
            driver
                .execute_capture(&prepared.sources().sources[0].prepared)
                .unwrap();
            prepared.mark_unknown().unwrap();
            candidate
        };
        let interrupted_id = interrupt_capture(&store, "fixture:capture");
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
        // Exact abandonment of an unpublished capture; changed refs must
        // remain untouched and the original hazard must survive failure.
        let abandon_capture = |store: &SessionStore, config: &Path, id: &str| {
            crate::checkpoint_capture::abandon(store, &actor, config, &request, id)
        };
        assert!(abandon_capture(
            &store,
            &approvals,
            "rpc:stdio:checkpoint:fixture-fresh-capture"
        )
        .is_err());
        let capture_candidate = interrupt_capture(&store, "fixture:abandon-capture");
        let private_ref = format!("refs/aw/checkpoints/{capture_candidate}/code");
        let original_oid = git(&executable, &repo, &["rev-parse", &private_ref]);
        assert!(store
            .checkpoint_abandon_capture_exact::<()>(
                &actor,
                &wrong_request,
                "fixture:abandon-capture",
                |_, _, _| panic!("wrong cleanup request reached provider")
            )
            .is_err());
        git(
            &executable,
            &repo,
            &["update-ref", &private_ref, &head, &original_oid],
        );
        assert!(abandon_capture(&store, &approvals, "fixture:abandon-capture").is_err());
        assert_eq!(git(&executable, &repo, &["rev-parse", &private_ref]), head);
        let mutations = SourceMutationSet::from_bindings(std::slice::from_ref(&binding)).unwrap();
        assert!(store
            .acquire_source_mutation_barrier(&mutations, "fixture:capture-still-unknown")
            .is_err());
        drop(store);
        git(
            &executable,
            &repo,
            &["update-ref", &private_ref, &original_oid, &head],
        );
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        abandon_capture(&store, &approvals, "fixture:abandon-capture").unwrap();
        assert!(git(&executable, &repo, &["for-each-ref", &private_ref]).is_empty());
        assert!(store
            .checkpoint_get_metadata(&actor, &capture_candidate)
            .is_err());
        assert!(invoke(&store, &approvals, "fixture:abandon-capture", false).is_err());

        // Restore the independently retained fresh checkpoint into named
        // layouts. No new grant, task, Agent activation or source recapture.
        let restore_request = CheckpointRestoreRequest {
            checkpoint: fresh.id.clone(),
            project: None,
            title: Some("Offline restored".into()),
            access: AgentHistoryForkAccess::Private,
            task: None,
        };
        let restore_path = approvals.join("restore.json");
        let destination = root.join("restored");
        let interrupted = root.join("restore-interrupted");
        for path in [&destination, &interrupted] {
            aw_platform::ensure_private_dir(path).unwrap();
        }
        let restore_config = |store: &SessionStore, target: &Path| {
            json!({
                "version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
                "store_generation":store.local_process_info().unwrap().store_generation,
                "manager":actor,"checkpoint":fresh.id,"binding":layout(target),
                "destination_identity":DirectoryIdentity::open(target).unwrap().identity().unwrap(),
                "repositories":[location],
            })
        };
        let mut restore_file = aw_platform::open_private_new(&restore_path).unwrap();
        restore_file
            .write_all(&serde_json::to_vec(&restore_config(&store, &destination)).unwrap())
            .unwrap();
        drop(restore_file);
        let invoke_restore = |store: &SessionStore, path: &Path, id: &str, recover: bool| {
            crate::checkpoint_restore::restore(store, &actor, path, &restore_request, id, recover)
        };
        let verify_restore =
            |store: &SessionStore, result: &aw_store::CheckpointRestoreResult, target: &Path| {
                assert_eq!(result.agent.admission, AgentAdmission::Held);
                assert_eq!(result.agent.project_ref, agent_before.project_ref);
                // Checkpoint restore creates a fresh local history root,
                // not a branch/parent-Agent relation.
                assert_ne!(result.agent.canonical_ref, *agent_ref);
                assert!(result.agent.parent_agent_ref.is_none());
                assert!(result.agent.history_content_digest.is_some());
                assert_eq!(
                    store
                        .sandbox_grant_status(&actor, &result.agent.canonical_ref)
                        .unwrap()
                        .revision,
                    0
                );
                let sandbox = store
                    .sandbox_get(&actor, result.agent.sandbox_ref.as_deref().unwrap())
                    .unwrap();
                assert_eq!(sandbox.tool_root, target);
                assert_eq!(sandbox.sources.len(), 1);
                assert_eq!(sandbox.sources[0].name, location.name);
                let execution = store
                    .agent_execution_binding(&actor, &result.agent.canonical_ref)
                    .unwrap();
                assert!(store
                    .actor_queue_list(&actor, execution.session_id())
                    .unwrap()
                    .is_empty());
                assert_eq!(
                    fs::read(target.join("code/tracked")).unwrap(),
                    b"dirty checkpoint"
                );
                assert_eq!(
                    git(&executable, &target.join("code"), &["rev-parse", "HEAD"]),
                    head
                );
            };
        fs::write(destination.join("foreign"), b"keep").unwrap();
        assert!(invoke_restore(
            &store,
            &restore_path,
            "rpc:stdio:checkpoint-restore:fixture-restore",
            false
        )
        .is_err());
        assert_eq!(fs::read(destination.join("foreign")).unwrap(), b"keep");
        assert!(!destination.join("code").exists());
        assert!(store
            .checkpoint_replay_restore(
                &actor,
                &restore_request,
                "rpc:stdio:checkpoint-restore:fixture-restore"
            )
            .unwrap()
            .is_none());
        fs::remove_file(destination.join("foreign")).unwrap();
        let held = lock_private_approval(&restore_path).unwrap();
        assert!(invoke_restore(
            &store,
            &restore_path,
            "rpc:stdio:checkpoint-restore:fixture-restore",
            false
        )
        .is_err());
        drop(held);
        let restore_port =
            |store: &SessionStore, approval: PathBuf| crate::source_construction_rpc::RestorePort {
                state: construction_state(store),
                approval,
            };
        let restore_params = json!({"id":fresh.id,"title":"Offline restored","access":"private"});
        assert!(restore_port(&store, restore_path.clone())
            .restore(
                "foreign",
                json!({"id":fresh.id,"project":"/d:foreign/i:foreign/p:foreign"})
            )
            .is_err());
        assert!(restore_port(&store, restore_path.clone())
            .restore("task", json!({"id":fresh.id,"task":""}))
            .is_err());
        assert!(restore_port(&store, restore_path.clone())
            .restore("inherit", json!({"id":fresh.id,"access":"inherit"}))
            .is_err());
        let restored_rpc = restore_port(&store, restore_path.clone())
            .restore("fixture-restore", restore_params.clone())
            .unwrap();
        let published = invoke_restore(
            &store,
            &restore_path,
            "rpc:stdio:checkpoint-restore:fixture-restore",
            false,
        )
        .unwrap();
        assert_eq!(restored_rpc["agent"]["ref"], published.agent.canonical_ref);
        assert_eq!(restored_rpc["sandbox"], json!(published.agent.sandbox_ref));
        assert_eq!(
            restore_port(&store, root.join("absent-approval"))
                .restore("fixture-restore", restore_params.clone())
                .unwrap(),
            restored_rpc
        );
        assert!(restore_port(&store, restore_path.clone())
            .restore(
                "fixture-restore",
                json!({"id":fresh.id,"title":"different"})
            )
            .is_err());
        verify_restore(&store, &published, &destination);
        assert_eq!(
            invoke_restore(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:checkpoint-restore:fixture-restore",
                false
            )
            .unwrap(),
            published
        );

        // Interrupt the original physical worktree creation before Store
        // publication, then use the exact configured recovery route.
        let interrupted_layout = layout(&interrupted);
        let interrupted_config = restore_config(&store, &interrupted);
        fs::write(
            &restore_path,
            serde_json::to_vec(&interrupted_config).unwrap(),
        )
        .unwrap();
        let interrupt_restore = |store: &SessionStore, target: &Path, id: &str| {
            let pending = store
                .checkpoint_prepare_restore(
                    &actor,
                    &restore_request,
                    &layout(target),
                    id,
                    "local",
                    |prepare_id, _, anchors| {
                        let [anchor] = anchors.anchors.as_slice() else {
                            panic!("expected exact captured source")
                        };
                        let anchor = GitSourceAnchor::try_from(anchor)?;
                        let selected = GitRestoreLocation {
                            name: location.name.clone(),
                            repository: location.repository.clone(),
                            repository_identity: location.repository_identity,
                            target: target.join("code"),
                            parent_identity: DirectoryIdentity::open(target)?.identity()?,
                        };
                        Ok(PreparedSourceSet {
                            prepare_id: prepare_id.to_owned(),
                            operation: SourceOperationKind::Restore,
                            sources: vec![NamedPreparedSource {
                                name: location.name.clone(),
                                prepared: driver
                                    .plan_restore_approved(&selected, &anchor, prepare_id)?,
                            }],
                        })
                    },
                )
                .unwrap();
            let candidate = pending.prepare_id().to_owned();
            let candidate_sandbox = pending.sandbox_ref().to_owned();
            pending.revalidate(&layout(target)).unwrap();
            driver
                .execute_restore(&pending.sources().sources[0].prepared)
                .unwrap();
            pending.mark_unknown().unwrap();
            (candidate, candidate_sandbox)
        };
        let (candidate, candidate_sandbox) = interrupt_restore(
            &store,
            &interrupted,
            "rpc:stdio:checkpoint-restore:fixture-restore-interrupted",
        );
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        assert!(invoke_restore(
            &store,
            &restore_path,
            "rpc:stdio:checkpoint-restore:fixture-restore-interrupted",
            false
        )
        .is_err());
        let mut wrong_layout = interrupted_layout.clone();
        wrong_layout.model_hint = "different model".into();
        assert!(store
            .checkpoint_inspect_captured_restore_exact(
                &actor,
                &restore_request,
                &wrong_layout,
                "rpc:stdio:checkpoint-restore:fixture-restore-interrupted",
                |_, _, _| panic!("wrong layout reached driver inspection")
            )
            .is_err());
        let wrong_request = CheckpointRestoreRequest {
            title: Some("Different title".into()),
            ..restore_request.clone()
        };
        assert!(store
            .checkpoint_inspect_captured_restore_exact(
                &actor,
                &wrong_request,
                &interrupted_layout,
                "rpc:stdio:checkpoint-restore:fixture-restore-interrupted",
                |_, _, _| panic!("wrong request reached driver inspection")
            )
            .is_err());
        let mut wrong_config = interrupted_config.clone();
        wrong_config["binding"] = json!(wrong_layout);
        fs::write(&restore_path, serde_json::to_vec(&wrong_config).unwrap()).unwrap();
        assert!(invoke_restore(
            &store,
            &restore_path,
            "rpc:stdio:checkpoint-restore:fixture-restore-interrupted",
            true
        )
        .is_err());
        fs::write(
            &restore_path,
            serde_json::to_vec(&interrupted_config).unwrap(),
        )
        .unwrap();
        fs::write(interrupted.join("foreign"), b"keep").unwrap();
        assert!(invoke_restore(
            &store,
            &restore_path,
            "rpc:stdio:checkpoint-restore:fixture-restore-interrupted",
            true
        )
        .is_err());
        assert_eq!(fs::read(interrupted.join("foreign")).unwrap(), b"keep");
        fs::remove_file(interrupted.join("foreign")).unwrap();
        // Ordinary RPC retry cannot rematerialize a Prepared/Unknown
        // candidate. Explicit offline recovery selects that same receipt.
        assert!(restore_port(&store, restore_path.clone())
            .restore("fixture-restore-interrupted", restore_params.clone())
            .is_err());
        let recovery_id = crate::source_construction_rpc::Operation::CheckpointRestore
            .offline_id("fixture-restore-interrupted", true, true)
            .unwrap();
        let recovered = invoke_restore(&store, &restore_path, &recovery_id, true).unwrap();
        verify_restore(&store, &recovered, &interrupted);
        assert_eq!(
            recovered.agent.history_content_digest,
            published.agent.history_content_digest
        );
        assert_eq!(
            recovered.agent.sandbox_ref.as_deref(),
            Some(candidate_sandbox.as_str())
        );
        assert_eq!(
            store
                .agent_execution_binding(&actor, &recovered.agent.canonical_ref)
                .unwrap()
                .session_id(),
            candidate
        );
        assert_eq!(store.agent_get(&actor, agent_ref).unwrap(), agent_before);
        assert_eq!(
            store.sandbox_grant_status(&actor, agent_ref).unwrap(),
            grant_before
        );

        // Abandon only an original unpublished materialization. A user edit
        // must prevent cleanup; exact repair and restart permit reconciliation.
        let abandon_restore = |store: &SessionStore, config: &Path, id: &str| {
            crate::checkpoint_restore::abandon(store, &actor, config, &restore_request, id)
        };
        assert!(abandon_restore(
            &store,
            &restore_path,
            "rpc:stdio:checkpoint-restore:fixture-restore"
        )
        .is_err());
        let abandon_root = root.join("restore-abandoned");
        aw_platform::ensure_private_dir(&abandon_root).unwrap();
        let abandon_config = restore_config(&store, &abandon_root);
        fs::write(&restore_path, serde_json::to_vec(&abandon_config).unwrap()).unwrap();
        let (_, abandoned_sandbox) =
            interrupt_restore(&store, &abandon_root, "fixture:abandon-restore");
        assert!(store
            .checkpoint_abandon_captured_restore_exact::<()>(
                &actor,
                &restore_request,
                &wrong_layout,
                "fixture:abandon-restore",
                |_, _, _| panic!("wrong cleanup layout reached provider")
            )
            .is_err());
        let changed = abandon_root.join("code/tracked");
        fs::write(&changed, b"independent edit").unwrap();
        assert!(abandon_restore(&store, &restore_path, "fixture:abandon-restore").is_err());
        assert_eq!(fs::read(&changed).unwrap(), b"independent edit");
        assert!(store
            .acquire_source_mutation_barrier(&mutations, "fixture:restore-still-unknown")
            .is_err());
        fs::write(&changed, b"dirty checkpoint").unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        abandon_restore(&store, &restore_path, "fixture:abandon-restore").unwrap();
        assert_eq!(fs::read_dir(&abandon_root).unwrap().count(), 0);
        assert!(store.sandbox_get(&actor, &abandoned_sandbox).is_err());
        assert!(invoke_restore(&store, &restore_path, "fixture:abandon-restore", false).is_err());
        assert!(store
            .checkpoint_replay_abandoned_captured_restore(
                &actor,
                &restore_request,
                "fixture:abandon-restore"
            )
            .unwrap());
        verify_restore(&store, &published, &destination);
        verify_restore(&store, &recovered, &interrupted);

        let reference = format!("refs/aw/checkpoints/{}/code", capture.id);
        let captured = git(&executable, &repo, &["rev-parse", &reference]);
        assert_ne!(captured, head);
        assert!(
            get_checkpoint(&store, true, &capture.id).unwrap()["sources"][0]
                .get("source")
                .is_none()
        );
        let mut metadata_config = metadata_config;
        let mut capture_identity = anchor_identity.clone();
        capture_identity["version"] = json!(captured);
        metadata_config["policy"]["sources"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "kind":"anchor","checkpoint":capture.id,"source":capture_identity,
            }));
        fs::write(
            &metadata_path,
            serde_json::to_vec(&metadata_config).unwrap(),
        )
        .unwrap();
        assert_eq!(
            get_checkpoint(&store, true, &capture.id).unwrap()["sources"][0]["source"],
            capture_identity
        );
        let config = json!({"version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
            "store_generation":store.local_process_info().unwrap().store_generation,"manager":actor,
            "checkpoint":capture.id,"repositories":[location]});
        let path = approvals.join("retention.json");
        let mut file = aw_platform::open_private_new(&path).unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        drop(file);
        use aw::headless::SourceRetention;
        let retention_state = |store: &SessionStore| crate::StdioSandboxPort {
            store: store.clone(),
            caller: actor.clone(),
            current_project_ref: project.clone(),
            source_metadata: None,
        };
        let retention_approval = crate::source_retention_rpc::RetentionApprovals {
            checkpoint_retention_approval: Some(path.clone()),
            ..Default::default()
        };
        let reconcile_checkpoint_rpc = |store: &SessionStore| {
            retention_approval.reconcile(
                SourceRetention::Checkpoint,
                &retention_state(store),
                &layout(&initial),
                json!({"id":capture.id}),
            )
        };
        assert!(
            reconcile_checkpoint_rpc(&store).is_err(),
            "live checkpoint is not cleanup authority"
        );
        assert!(
            retention_approval
                .reconcile(
                    SourceRetention::Checkpoint,
                    &retention_state(&store),
                    &layout(&initial),
                    json!({"id":fresh.id}),
                )
                .is_err(),
            "independent approval names a different checkpoint"
        );
        let approval = Approval::open(&path, &actor).unwrap();
        assert!(Approval::open(&path, &actor).is_err());
        assert!(
            approval.reconcile(&store).is_err(),
            "live checkpoint must not release"
        );
        store.checkpoint_delete(&actor, &capture.id).unwrap();
        assert_eq!(
            get_checkpoint(&store, true, &capture.id).unwrap()["sources"],
            json!([])
        );
        let before = store
            .agent_get(&actor, &restored.agent.canonical_ref)
            .unwrap();
        drop(approval);
        // The currently bound controller's layout cannot serve as
        // physical retention backing, even with valid target approval.
        assert!(retention_approval
            .reconcile(
                SourceRetention::Checkpoint,
                &retention_state(&store),
                &layout(&repo),
                json!({"id":capture.id}),
            )
            .is_err());
        let controller_alias = root.join("controller-alias");
        std::os::unix::fs::symlink(&repo, &controller_alias).unwrap();
        assert!(retention_approval
            .reconcile(
                SourceRetention::Checkpoint,
                &retention_state(&store),
                &layout(&controller_alias),
                json!({"id":capture.id}),
            )
            .is_err());
        let mut wrong_project = retention_state(&store);
        wrong_project.current_project_ref = "/d:other/i:other/p:other".into();
        assert!(retention_approval
            .reconcile(
                SourceRetention::Checkpoint,
                &wrong_project,
                &layout(&initial),
                json!({"id":capture.id}),
            )
            .is_err());
        drop(wrong_project);
        assert_eq!(
            git(&executable, &repo, &["rev-parse", &reference]),
            captured
        );
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
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        assert_eq!(
            reconcile_checkpoint_rpc(&store).unwrap(),
            json!({"status":"settled"})
        );
        // Same resource operation is a factual no-op, with no second
        // physical inspection. The original approval remains necessary.
        assert_eq!(
            reconcile_checkpoint_rpc(&store).unwrap(),
            json!({"status":"settled"})
        );
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
        // Retire only a logically closed, originally managed Sandbox. The
        // fixture owns every non-Store effect; the production cleanup command
        // never requests or fabricates Agent/Sandbox closure.
        let sandbox_ref = published.agent.sandbox_ref.as_deref().unwrap();
        let open_sandbox = store.sandbox_get(&actor, sandbox_ref).unwrap();
        let sandbox_path = approvals.join("sandbox-retention.json");
        let sandbox_config = json!({
            "version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor,"sandbox":sandbox_ref,"sandbox_rev":open_sandbox.rev + 1,
            "destination_root":destination,
            "destination_identity":DirectoryIdentity::open(&destination).unwrap().identity().unwrap(),
            "repositories":[location],
        });
        let mut sandbox_file = aw_platform::open_private_new(&sandbox_path).unwrap();
        sandbox_file
            .write_all(&serde_json::to_vec(&sandbox_config).unwrap())
            .unwrap();
        drop(sandbox_file);
        let sandbox_retention_approval = crate::source_retention_rpc::RetentionApprovals {
            sandbox_retention_approval: Some(sandbox_path.clone()),
            ..Default::default()
        };
        let reconcile_sandbox_rpc = |store: &SessionStore| {
            sandbox_retention_approval.reconcile(
                SourceRetention::Sandbox,
                &retention_state(store),
                &layout(&initial),
                json!({"ref":sandbox_ref,"rev":open_sandbox.rev+1}),
            )
        };
        let reconcile_sandbox = |store: &SessionStore| {
            crate::sandbox_retention::Approval::open(&sandbox_path, &actor)?.reconcile(store)
        };
        assert!(reconcile_sandbox_rpc(&store).is_err());
        assert!(reconcile_sandbox(&store).is_err());
        assert!(destination.join("code").exists());
        store
            .agent_request_close(&actor, &published.agent.canonical_ref, None)
            .unwrap();
        let closed_agent = store
            .agent_finish_close(&actor, &published.agent.canonical_ref)
            .unwrap();
        let closed = store
            .sandbox_close(&actor, sandbox_ref, Some(open_sandbox.rev))
            .unwrap();
        assert_eq!(closed.rev, open_sandbox.rev + 1);
        assert!(sandbox_retention_approval
            .reconcile(
                SourceRetention::Sandbox,
                &retention_state(&store),
                &layout(&destination),
                json!({"ref":sandbox_ref,"rev":closed.rev}),
            )
            .is_err());
        assert!(sandbox_retention_approval
            .reconcile(
                SourceRetention::Sandbox,
                &retention_state(&store),
                &layout(&initial),
                json!({"ref":sandbox_ref,"rev":closed.rev+1}),
            )
            .is_err());
        assert!(sandbox_retention_approval
            .reconcile(
                SourceRetention::Sandbox,
                &retention_state(&store),
                &layout(&initial),
                json!({"ref":"/d:foreign/i:foreign/p:foreign/s:foreign","rev":closed.rev}),
            )
            .is_err());
        let mut stale = sandbox_config.clone();
        stale["sandbox_rev"] = json!(closed.rev + 1);
        fs::write(&sandbox_path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(reconcile_sandbox(&store).is_err());
        fs::write(&sandbox_path, serde_json::to_vec(&sandbox_config).unwrap()).unwrap();
        let foreign = destination.join("foreign");
        fs::write(&foreign, b"keep").unwrap();
        assert!(reconcile_sandbox(&store).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert_eq!(store.sandbox_get(&actor, sandbox_ref).unwrap(), closed);
        store
            .acquire_source_mutation_barrier(
                &mutations,
                "fixture:closed-sandbox-preflight-no-hazard",
            )
            .unwrap()
            .settle(SourceMutationOutcome::Settled)
            .unwrap();
        fs::remove_file(&foreign).unwrap();
        let edited = destination.join("code/tracked");
        fs::write(&edited, b"new work").unwrap();
        assert!(reconcile_sandbox(&store).is_err());
        assert_eq!(fs::read(&edited).unwrap(), b"new work");
        assert_eq!(store.sandbox_get(&actor, sandbox_ref).unwrap(), closed);
        assert!(store
            .acquire_source_mutation_barrier(&mutations, "fixture:closed-sandbox-still-unknown")
            .is_err());
        assert!(store
            .create(&actor, "reserved layout", layout(&destination))
            .is_err());
        fs::write(&edited, b"dirty checkpoint").unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let release_rpc = reconcile_sandbox_rpc(&store).unwrap();
        let released = reconcile_sandbox(&store).unwrap();
        assert_eq!(release_rpc["ref"], released.canonical_ref);
        assert_eq!(release_rpc["rev"], released.rev);
        assert_eq!(release_rpc["sources"], json!([]));
        assert_eq!(reconcile_sandbox_rpc(&store).unwrap(), release_rpc);
        assert_eq!(released.lifecycle, aw_store::SandboxLifecycle::Closed);
        assert_eq!(released.rev, closed.rev + 1);
        assert!(released.sources.is_empty());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_eq!(
            store
                .agent_get(&actor, &published.agent.canonical_ref)
                .unwrap(),
            closed_agent
        );
        assert_eq!(reconcile_sandbox(&store).unwrap(), released);
        assert!(store
            .sandbox_reconcile_closed_sources_fenced(
                &actor,
                sandbox_ref,
                Some(released.rev),
                |_, _| -> aw::Result<()> { panic!("wrong release revision authorized") },
                |_, _, _| panic!("wrong release revision reconciled")
            )
            .is_err());
        verify_restore(&store, &recovered, &interrupted);
        assert!(!git(
            &executable,
            &repo,
            &[
                "for-each-ref",
                &format!("refs/aw/checkpoints/{}/code", fresh.id)
            ]
        )
        .is_empty());
        // Standalone derivation uses the same original construction owner,
        // with no history/checkpoint or Agent manufactured. Qualify the
        // actual offline consumer, including exact interruption cleanup.
        let parent = store.sandbox_get(&actor, &sandbox.canonical_ref).unwrap();
        let derive_path = approvals.join("derive.json");
        let child_root = root.join("derived-child");
        let pending_root = root.join("derive-pending");
        let discard_root = root.join("derive-discard");
        for target in [&child_root, &pending_root, &discard_root] {
            aw_platform::ensure_private_dir(target).unwrap();
        }
        let derive_config = |store: &SessionStore, target: &Path| {
            json!({"version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
                "store_generation":store.local_process_info().unwrap().store_generation,
                "manager":actor,"parent":parent.canonical_ref,"parent_rev":parent.rev,
                "binding":layout(target),
                "destination_identity":DirectoryIdentity::open(target).unwrap().identity().unwrap(),
                "repositories":[location]})
        };
        let mut derive_file = aw_platform::open_private_new(&derive_path).unwrap();
        let config = derive_config(&store, &child_root);
        let mut stale = config.clone();
        stale["parent_rev"] = json!(parent.rev + 1);
        derive_file
            .write_all(&serde_json::to_vec(&stale).unwrap())
            .unwrap();
        drop(derive_file);
        let derive = |store: &SessionStore, path: &Path, id: &str, recover: bool| {
            crate::sandbox_derive::derive(store, &actor, path, &parent.canonical_ref, id, recover)
        };
        let abandon_derive = |store: &SessionStore, path: &Path, id: &str| {
            crate::sandbox_derive::abandon(store, &actor, path, &parent.canonical_ref, id)
        };
        let agents_before = store.agent_list(&actor, project, None, 64).unwrap();
        let checkpoints_before = store
            .checkpoint_list_metadata(&actor, agent_ref, None, 64)
            .unwrap();
        assert!(derive(
            &store,
            &derive_path,
            "rpc:stdio:sandbox-create:fixture-derive",
            false
        )
        .is_err());
        assert!(store
            .sandbox_prepare_derive_exact(
                &actor,
                project,
                &parent.canonical_ref,
                parent.rev + 1,
                &layout(&child_root),
                "rpc:stdio:sandbox-create:fixture-derive",
                |_, _, _| panic!("stale parent reached planner")
            )
            .is_err());
        fs::write(&derive_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let lease = lock_private_approval(&derive_path).unwrap();
        assert!(derive(
            &store,
            &derive_path,
            "rpc:stdio:sandbox-create:fixture-derive",
            false
        )
        .is_err());
        drop(lease);
        let foreign = child_root.join("foreign");
        fs::write(&foreign, b"keep").unwrap();
        assert!(derive(
            &store,
            &derive_path,
            "rpc:stdio:sandbox-create:fixture-derive",
            false
        )
        .is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        fs::remove_file(foreign).unwrap();
        let sandbox_port = |store: &SessionStore| crate::StdioSandboxPort {
            store: store.clone(),
            caller: actor.clone(),
            current_project_ref: project.clone(),
            source_metadata: None,
        };
        let derive_approval = crate::source_construction_rpc::Approvals {
            sandbox_derive_approval: Some(derive_path.clone()),
            ..Default::default()
        };
        let derive_params = json!({"from":parent.canonical_ref});
        assert!(derive_approval
            .create_sandbox(
                &sandbox_port(&store),
                "cross-project",
                json!({"from":"/d:foreign/i:foreign/p:foreign/s:foreign"})
            )
            .is_err());
        assert!(derive_approval
            .create_sandbox(&sandbox_port(&store), "wrong-mode", json!({}))
            .is_err());
        let derived_rpc = derive_approval
            .create_sandbox(
                &sandbox_port(&store),
                "fixture-derive",
                derive_params.clone(),
            )
            .unwrap();
        let child = derive(
            &store,
            &derive_path,
            "rpc:stdio:sandbox-create:fixture-derive",
            false,
        )
        .unwrap();
        assert_eq!(derived_rpc["ref"], child.canonical_ref);
        assert_eq!(derived_rpc["parent"], parent.canonical_ref);
        assert!(derived_rpc["sources"][0].get("source_ref").is_none());
        let absent_derive = crate::source_construction_rpc::Approvals {
            sandbox_derive_approval: Some(root.join("absent-approval")),
            ..Default::default()
        };
        assert_eq!(
            absent_derive
                .create_sandbox(&sandbox_port(&store), "fixture-derive", derive_params)
                .unwrap(),
            derived_rpc
        );
        assert_eq!(
            child.parent_sandbox_ref.as_deref(),
            Some(parent.canonical_ref.as_str())
        );
        assert_eq!(child.sources.len(), 1);
        assert_eq!(
            fs::read(child_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert_eq!(
            derive(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:sandbox-create:fixture-derive",
                false
            )
            .unwrap(),
            child
        );
        assert!(abandon_derive(
            &store,
            &derive_path,
            "rpc:stdio:sandbox-create:fixture-derive"
        )
        .is_err());

        let interrupt_derive = |store: &SessionStore, target: &Path, id: &str| {
            let prepared = store
                .sandbox_prepare_derive_exact(
                    &actor,
                    project,
                    &parent.canonical_ref,
                    parent.rev,
                    &layout(target),
                    id,
                    |prepare_id, _, input| {
                        let aw_store::SandboxCreateSources::Parent(bindings) = input else {
                            panic!("expected exact parent")
                        };
                        Ok(PreparedSourceSet {
                            prepare_id: prepare_id.into(),
                            operation: SourceOperationKind::Derive,
                            sources: vec![NamedPreparedSource {
                                name: location.name.clone(),
                                prepared: driver.plan_derive_approved(
                                    &GitRestoreLocation {
                                        name: location.name.clone(),
                                        repository: location.repository.clone(),
                                        repository_identity: location.repository_identity,
                                        target: target.join("code"),
                                        parent_identity: DirectoryIdentity::open(target)?
                                            .identity()?,
                                    },
                                    &bindings[0].binding,
                                    prepare_id,
                                )?,
                            }],
                        })
                    },
                )
                .unwrap();
            let candidate = prepared.sandbox_ref().to_owned();
            prepared.revalidate(&layout(target)).unwrap();
            driver
                .execute_derive(&prepared.sources().sources[0].prepared)
                .unwrap();
            prepared.mark_unknown().unwrap();
            candidate
        };
        let pending_ref = interrupt_derive(
            &store,
            &pending_root,
            "rpc:stdio:sandbox-create:fixture-derive-pending",
        );
        assert!(store
            .sandbox_inspect_derive_exact(
                &actor,
                project,
                &parent.canonical_ref,
                parent.rev,
                &layout(&discard_root),
                "rpc:stdio:sandbox-create:fixture-derive-pending",
                |_, _, _| panic!("wrong recovery layout reached inspector")
            )
            .is_err());
        assert!(store
            .sandbox_inspect_derive_exact(
                &actor,
                project,
                &parent.canonical_ref,
                parent.rev + 1,
                &layout(&pending_root),
                "rpc:stdio:sandbox-create:fixture-derive-pending",
                |_, _, _| panic!("wrong original parent revision reached inspector")
            )
            .is_err());
        fs::write(
            &derive_path,
            serde_json::to_vec(&derive_config(&store, &discard_root)).unwrap(),
        )
        .unwrap();
        assert!(derive(
            &store,
            &derive_path,
            "rpc:stdio:sandbox-create:fixture-derive-pending",
            true
        )
        .is_err());
        fs::write(
            &derive_path,
            serde_json::to_vec(&derive_config(&store, &pending_root)).unwrap(),
        )
        .unwrap();
        assert!(derive(
            &store,
            &derive_path,
            "rpc:stdio:sandbox-create:fixture-derive-pending",
            false
        )
        .is_err());
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        assert!(derive_approval
            .create_sandbox(
                &sandbox_port(&store),
                "fixture-derive-pending",
                json!({"from":parent.canonical_ref})
            )
            .is_err());
        let recovery_id = crate::source_construction_rpc::Operation::SandboxDerive
            .offline_id("fixture-derive-pending", true, true)
            .unwrap();
        let derived = derive(&store, &derive_path, &recovery_id, true).unwrap();
        assert_eq!(derived.canonical_ref, pending_ref);
        assert_eq!(
            fs::read(pending_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );

        let discarded = interrupt_derive(&store, &discard_root, "fixture:derive-discard");
        fs::write(
            &derive_path,
            serde_json::to_vec(&derive_config(&store, &discard_root)).unwrap(),
        )
        .unwrap();
        let changed = discard_root.join("code/tracked");
        fs::write(&changed, b"independent edit").unwrap();
        assert!(abandon_derive(&store, &derive_path, "fixture:derive-discard").is_err());
        assert_eq!(fs::read(&changed).unwrap(), b"independent edit");
        assert!(store
            .acquire_source_mutation_barrier(&mutations, "fixture:derive-unknown")
            .is_err());
        fs::write(&changed, b"dirty checkpoint").unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        abandon_derive(&store, &derive_path, "fixture:derive-discard").unwrap();
        assert!(store.sandbox_get(&actor, &discarded).is_err());
        assert_eq!(fs::read_dir(&discard_root).unwrap().count(), 0);
        assert!(derive(&store, &derive_path, "fixture:derive-discard", false).is_err());
        assert_eq!(
            store.sandbox_get(&actor, &parent.canonical_ref).unwrap(),
            parent
        );
        assert_eq!(
            store.agent_list(&actor, project, None, 64).unwrap(),
            agents_before
        );
        assert_eq!(
            store
                .checkpoint_list_metadata(&actor, agent_ref, None, 64)
                .unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fs::read(child_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert_eq!(
            fs::read(pending_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );

        // Independently select an existing retained Git version as the
        // Project baseline. It is not a history restore or a parent copy.
        use aw_agent_core::{SourceCoverage, SourceSet};
        use aw_git::source_driver::GitSourceAnchor;
        use aw_store::{SandboxCreateFrom, SandboxCreateSources, SandboxRecoverySelection};
        let retained_anchor = GitSourceAnchor {
            driver: binding.driver.clone(),
            anchor_schema: fresh.sources[0].anchor_schema,
            source_ref: binding.source_ref.clone(),
            name: location.name.clone(),
            access: binding.access,
            checkpoint_policy: binding.checkpoint_policy,
            checkpoint_id: fresh.id.clone(),
            base_commit: head.clone(),
            captured_commit: fresh_version.clone(),
        };
        let baseline = SourceSet {
            coverage: SourceCoverage::Selected,
            anchors: vec![(&retained_anchor).try_into().unwrap()],
        };
        let baseline_path = approvals.join("baseline.json");
        let baseline_root = root.join("versioned-baseline");
        let baseline_pending = root.join("baseline-pending");
        let baseline_discard = root.join("baseline-discard");
        for target in [&baseline_root, &baseline_pending, &baseline_discard] {
            aw_platform::ensure_private_dir(target).unwrap();
        }
        let baseline_config = |target: &Path| {
            json!({"version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
                "store_generation":store.local_process_info().unwrap().store_generation,
                "manager":actor,"project":project,"baseline":baseline,"binding":layout(target),
                "destination_identity":DirectoryIdentity::open(target).unwrap().identity().unwrap(),
                "repositories":[location]})
        };
        let config = baseline_config(&baseline_root);
        let pending_config = baseline_config(&baseline_pending);
        let discard_config = baseline_config(&baseline_discard);
        let create_baseline = |store: &SessionStore, path: &Path, id: &str, recover: bool| {
            crate::sandbox_create::create(store, &actor, path, project, id, recover)
        };
        let abandon_baseline = |store: &SessionStore, path: &Path, id: &str| {
            crate::sandbox_create::abandon(store, &actor, path, project, id)
        };
        let mut wrong = config.clone();
        wrong["baseline"]["anchors"][0]["identity"] = json!(head);
        let mut baseline_file = aw_platform::open_private_new(&baseline_path).unwrap();
        baseline_file
            .write_all(&serde_json::to_vec(&wrong).unwrap())
            .unwrap();
        drop(baseline_file);
        assert!(create_baseline(
            &store,
            &baseline_path,
            "rpc:stdio:sandbox-create:fixture-baseline-create",
            false
        )
        .is_err());
        assert_eq!(fs::read_dir(&baseline_root).unwrap().count(), 0);
        fs::write(&baseline_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let lease = lock_private_approval(&baseline_path).unwrap();
        assert!(create_baseline(
            &store,
            &baseline_path,
            "rpc:stdio:sandbox-create:fixture-baseline-create",
            false
        )
        .is_err());
        drop(lease);

        let retained_refs = git(
            &executable,
            &repo,
            &["for-each-ref", "refs/aw/checkpoints/"],
        );
        fs::write(repo.join("tracked"), b"current mutable parent").unwrap();
        let baseline_approval = crate::source_construction_rpc::Approvals {
            sandbox_create_approval: Some(baseline_path.clone()),
            ..Default::default()
        };
        assert!(baseline_approval
            .create_sandbox(
                &sandbox_port(&store),
                "foreign",
                json!({"project":"/d:foreign/i:foreign/p:foreign"})
            )
            .is_err());
        let baseline_rpc = baseline_approval
            .create_sandbox(&sandbox_port(&store), "fixture-baseline-create", json!({}))
            .unwrap();
        let standalone = create_baseline(
            &store,
            &baseline_path,
            "rpc:stdio:sandbox-create:fixture-baseline-create",
            false,
        )
        .unwrap();
        assert_eq!(baseline_rpc["ref"], standalone.canonical_ref);
        assert!(baseline_rpc["parent"].is_null());
        assert!(baseline_rpc["sources"][0].get("source_ref").is_none());
        let absent_baseline = crate::source_construction_rpc::Approvals {
            sandbox_create_approval: Some(root.join("absent-approval")),
            ..Default::default()
        };
        assert_eq!(
            absent_baseline
                .create_sandbox(
                    &sandbox_port(&store),
                    "fixture-baseline-create",
                    json!({"project":project})
                )
                .unwrap(),
            baseline_rpc
        );
        let both = crate::source_construction_rpc::Approvals {
            sandbox_derive_approval: Some(derive_path.clone()),
            ..baseline_approval
        };
        assert!(both
            .create_sandbox(
                &sandbox_port(&store),
                "fixture-baseline-create",
                json!({"from":parent.canonical_ref})
            )
            .is_err());
        assert!(standalone.parent_sandbox_ref.is_none());
        assert_eq!(standalone.sources.len(), 1);
        assert_eq!(
            fs::read(baseline_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert_eq!(
            fs::read(repo.join("tracked")).unwrap(),
            b"current mutable parent"
        );
        fs::write(repo.join("tracked"), b"dirty checkpoint").unwrap();
        assert_eq!(
            create_baseline(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:sandbox-create:fixture-baseline-create",
                false
            )
            .unwrap(),
            standalone
        );
        assert!(abandon_baseline(
            &store,
            &baseline_path,
            "rpc:stdio:sandbox-create:fixture-baseline-create"
        )
        .is_err());

        let interrupt_baseline = |store: &SessionStore, target: &Path, id: &str| {
            let prepared = store
                .sandbox_prepare_create(
                    &actor,
                    project,
                    &layout(target),
                    SandboxCreateFrom::Baseline(baseline.clone()),
                    id,
                    |prepare_id, _, input| {
                        assert!(
                            matches!(input, SandboxCreateSources::Baseline(set) if set == &baseline)
                        );
                        Ok(PreparedSourceSet {
                            prepare_id: prepare_id.into(),
                            operation: SourceOperationKind::Restore,
                            sources: vec![NamedPreparedSource {
                                name: location.name.clone(),
                                prepared: driver.plan_restore_approved(
                                    &GitRestoreLocation {
                                        name: location.name.clone(),
                                        repository: location.repository.clone(),
                                        repository_identity: location.repository_identity,
                                        target: target.join("code"),
                                        parent_identity: DirectoryIdentity::open(target)?
                                            .identity()?,
                                    },
                                    &retained_anchor,
                                    prepare_id,
                                )?,
                            }],
                        })
                    },
                )
                .unwrap();
            let candidate = prepared.sandbox_ref().to_owned();
            prepared.revalidate(&layout(target)).unwrap();
            driver
                .execute_restore(&prepared.sources().sources[0].prepared)
                .unwrap();
            prepared.mark_unknown().unwrap();
            candidate
        };
        let original = interrupt_baseline(&store, &baseline_pending, "fixture:baseline-pending");
        let mut wrong_selection = baseline.clone();
        wrong_selection.anchors[0].identity = head.clone();
        assert!(store
            .sandbox_inspect_create_exact(
                &actor,
                project,
                "fixture:baseline-pending",
                &layout(&baseline_pending),
                SandboxRecoverySelection::Baseline(&wrong_selection),
                |_, _, _| panic!("changed baseline reached inspector")
            )
            .is_err());
        assert!(store
            .sandbox_inspect_create_exact(
                &actor,
                project,
                "fixture:baseline-pending",
                &layout(&baseline_discard),
                SandboxRecoverySelection::Baseline(&baseline),
                |_, _, _| panic!("changed baseline layout reached inspector")
            )
            .is_err());
        let mut wrong = pending_config.clone();
        wrong["baseline"]["anchors"][0]["identity"] = json!(head);
        fs::write(&baseline_path, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(create_baseline(&store, &baseline_path, "fixture:baseline-pending", true).is_err());
        assert!(abandon_baseline(&store, &baseline_path, "fixture:baseline-pending").is_err());
        fs::write(&baseline_path, serde_json::to_vec(&pending_config).unwrap()).unwrap();
        assert!(
            create_baseline(&store, &baseline_path, "fixture:baseline-pending", false).is_err()
        );
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let baseline_recovered =
            create_baseline(&store, &baseline_path, "fixture:baseline-pending", true).unwrap();
        assert_eq!(baseline_recovered.canonical_ref, original);
        let unpublished = interrupt_baseline(&store, &baseline_discard, "fixture:baseline-discard");
        fs::write(&baseline_path, serde_json::to_vec(&discard_config).unwrap()).unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        abandon_baseline(&store, &baseline_path, "fixture:baseline-discard").unwrap();
        assert!(store.sandbox_get(&actor, &unpublished).is_err());
        assert_eq!(fs::read_dir(&baseline_discard).unwrap().count(), 0);
        assert!(
            create_baseline(&store, &baseline_path, "fixture:baseline-discard", false).is_err()
        );
        assert_eq!(
            git(
                &executable,
                &repo,
                &["for-each-ref", "refs/aw/checkpoints/"]
            ),
            retained_refs
        );
        assert_eq!(
            fs::read(baseline_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert_eq!(
            fs::read(baseline_pending.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert_eq!(
            store.agent_list(&actor, project, None, 64).unwrap(),
            agents_before
        );
        assert_eq!(
            store
                .checkpoint_list_metadata(&actor, agent_ref, None, 64)
                .unwrap(),
            checkpoints_before
        );
        assert_eq!(
            store.sandbox_get(&actor, &parent.canonical_ref).unwrap(),
            parent
        );
        assert_eq!(git(&executable, &repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(repo.join("tracked")).unwrap(), b"dirty checkpoint");

        // Qualify the production first-source attachment under the original
        // empty-layout reservation. The existing Host test covers the
        // complete multi-source, replacement and partial release owners.
        use aw_store::SandboxMaterializationRequest;
        let attach_root = root.join("attach-first-source");
        let attach_pending_root = root.join("attach-pending");
        for target in [&attach_root, &attach_pending_root] {
            aw_platform::ensure_private_dir(target).unwrap();
        }
        let empty = store
            .sandbox_create_fresh(&actor, project, &layout(&attach_root))
            .unwrap();
        let pending_empty = store
            .sandbox_create_fresh(&actor, project, &layout(&attach_pending_root))
            .unwrap();
        let anchor = &baseline.anchors[0];
        let attach_config = |snapshot: &aw_store::SandboxSnapshot| {
            json!({"version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
                "store_generation":store.local_process_info().unwrap().store_generation,
                "manager":actor,"sandbox":snapshot.canonical_ref,"sandbox_rev":snapshot.rev,
                "anchor":anchor,"destination_root":snapshot.tool_root,
                "destination_identity":DirectoryIdentity::open(&snapshot.tool_root).unwrap().identity().unwrap(),
                "repositories":[location]})
        };
        let attach_path = approvals.join("attach.json");
        let pending_attach_path = approvals.join("attach-pending.json");
        let config = attach_config(&empty);
        let pending_config = attach_config(&pending_empty);
        let mut file = aw_platform::open_private_new(&attach_path).unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        drop(file);
        let mut file = aw_platform::open_private_new(&pending_attach_path).unwrap();
        file.write_all(&serde_json::to_vec(&pending_config).unwrap())
            .unwrap();
        drop(file);
        let attach = |store: &SessionStore, path: &Path, sandbox: &str, id: &str, recover: bool| {
            crate::sandbox_source_attach::attach(store, &actor, path, sandbox, id, recover)
        };
        let abandon_attach = |store: &SessionStore, path: &Path, sandbox: &str, id: &str| {
            crate::sandbox_source_attach::abandon(store, &actor, path, sandbox, id)
        };
        let mut stale = config.clone();
        stale["sandbox_rev"] = json!(empty.rev + 1);
        fs::write(&attach_path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach",
            false
        )
        .is_err());
        fs::write(&attach_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let held = lock_private_approval(&attach_path).unwrap();
        assert!(attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach",
            false
        )
        .is_err());
        drop(held);
        let foreign = attach_root.join("foreign");
        fs::write(&foreign, b"keep").unwrap();
        assert!(attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach",
            false
        )
        .is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        fs::remove_file(&foreign).unwrap();
        assert!(store
            .sandbox_prepare_attachment(
                &actor,
                &parent.canonical_ref,
                parent.rev,
                "fixture:bound-attach",
                anchor.clone(),
                |_, _| panic!("bound Sandbox reached planner")
            )
            .is_err());

        let interrupt_attach =
            |store: &SessionStore, snapshot: &aw_store::SandboxSnapshot, id: &str| {
                let prepared = store
                    .sandbox_prepare_attachment(
                        &actor,
                        &snapshot.canonical_ref,
                        snapshot.rev,
                        id,
                        anchor.clone(),
                        |_, prepare_id| {
                            driver.plan_restore_approved(
                                &GitRestoreLocation {
                                    name: location.name.clone(),
                                    repository: location.repository.clone(),
                                    repository_identity: location.repository_identity,
                                    target: snapshot.tool_root.join("code"),
                                    parent_identity: DirectoryIdentity::open(&snapshot.tool_root)?
                                        .identity()?,
                                },
                                &retained_anchor,
                                prepare_id,
                            )
                        },
                    )
                    .unwrap();
                prepared.revalidate().unwrap();
                driver.execute_restore(prepared.source()).unwrap();
                prepared.mark_unknown().unwrap();
            };
        interrupt_attach(&store, &empty, "fixture:attach-abandon");
        let edited = attach_root.join("code/tracked");
        fs::write(&edited, b"independent edit").unwrap();
        assert!(abandon_attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach-abandon"
        )
        .is_err());
        assert_eq!(fs::read(&edited).unwrap(), b"independent edit");
        assert_eq!(
            store.sandbox_get(&actor, &empty.canonical_ref).unwrap(),
            empty
        );
        assert!(store
            .create(&actor, "reserved first-source", layout(&attach_root))
            .is_err());
        fs::write(&edited, b"dirty checkpoint").unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        abandon_attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach-abandon",
        )
        .unwrap();
        assert!(fs::read_dir(&attach_root).unwrap().next().is_none());
        assert!(attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach-abandon",
            false
        )
        .is_err());
        let attached = attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach",
            false,
        )
        .unwrap();
        assert_eq!(attached.rev, empty.rev + 1);
        assert_eq!(attached.sources.len(), 1);
        assert_eq!(attached.sources[0].binding_rev, attached.rev);
        assert_eq!(attached.layout_digest, empty.layout_digest);
        assert_eq!(
            fs::read(attach_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        // Terminal replay never takes custody of the subsequently attached source.
        abandon_attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach-abandon",
        )
        .unwrap();
        assert_eq!(
            attach(
                &store,
                &attach_path,
                &empty.canonical_ref,
                "fixture:attach",
                false
            )
            .unwrap(),
            attached
        );
        assert!(
            abandon_attach(&store, &attach_path, &empty.canonical_ref, "fixture:attach").is_err()
        );

        interrupt_attach(&store, &pending_empty, "fixture:attach-pending");
        let mut wrong = anchor.clone();
        wrong.identity = head.clone();
        for selection in [
            SandboxMaterializationRequest {
                expected_rev: pending_empty.rev,
                anchor: &wrong,
                replace: false,
            },
            SandboxMaterializationRequest {
                expected_rev: pending_empty.rev + 1,
                anchor,
                replace: false,
            },
            SandboxMaterializationRequest {
                expected_rev: pending_empty.rev,
                anchor,
                replace: true,
            },
        ] {
            assert!(store
                .sandbox_inspect_attachment_exact(
                    &actor,
                    &pending_empty.canonical_ref,
                    "fixture:attach-pending",
                    selection,
                    |_, _, _| panic!("wrong request reached inspector")
                )
                .is_err());
        }
        let mut wrong_config = pending_config.clone();
        wrong_config["anchor"]["identity"] = json!(head);
        fs::write(
            &pending_attach_path,
            serde_json::to_vec(&wrong_config).unwrap(),
        )
        .unwrap();
        assert!(attach(
            &store,
            &pending_attach_path,
            &pending_empty.canonical_ref,
            "fixture:attach-pending",
            true
        )
        .is_err());
        assert!(abandon_attach(
            &store,
            &pending_attach_path,
            &pending_empty.canonical_ref,
            "fixture:attach-pending"
        )
        .is_err());
        fs::write(
            &pending_attach_path,
            serde_json::to_vec(&pending_config).unwrap(),
        )
        .unwrap();
        assert!(attach(
            &store,
            &pending_attach_path,
            &pending_empty.canonical_ref,
            "fixture:attach-pending",
            false
        )
        .is_err());
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let attached_recovered = attach(
            &store,
            &pending_attach_path,
            &pending_empty.canonical_ref,
            "fixture:attach-pending",
            true,
        )
        .unwrap();
        assert_eq!(attached_recovered.rev, pending_empty.rev + 1);
        assert_eq!(
            fs::read(attach_pending_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert_eq!(
            store.agent_list(&actor, project, None, 64).unwrap(),
            agents_before
        );
        assert_eq!(
            store
                .checkpoint_list_metadata(&actor, agent_ref, None, 64)
                .unwrap(),
            checkpoints_before
        );
        assert_eq!(
            git(
                &executable,
                &repo,
                &["for-each-ref", "refs/aw/checkpoints/"]
            ),
            retained_refs
        );
        assert_eq!(git(&executable, &repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(repo.join("tracked")).unwrap(), b"dirty checkpoint");

        // Independently retained version from a different actual backing.
        // The fixture owns this private origin and retention throughout.
        use crate::sandbox_source_attach::MaterializationMode::{Continue, Fresh, Recover};
        let replacement_repo = root.join("replacement-origin");
        aw_platform::ensure_private_dir(&replacement_repo).unwrap();
        git(
            &executable,
            &replacement_repo,
            &["init", "--initial-branch=main"],
        );
        git(
            &executable,
            &replacement_repo,
            &["config", "user.name", "Fixture"],
        );
        git(
            &executable,
            &replacement_repo,
            &["config", "user.email", "fixture@example.invalid"],
        );
        fs::write(replacement_repo.join("tracked"), b"replacement version").unwrap();
        git(&executable, &replacement_repo, &["add", "tracked"]);
        git(
            &executable,
            &replacement_repo,
            &["commit", "--no-gpg-sign", "-m", "fixture"],
        );
        let replacement_binding = driver
            .binding(
                &replacement_repo,
                location.name.clone(),
                SourceAccess::ReadWrite,
                CheckpointPolicy::Required,
            )
            .unwrap();
        let replacement_capture = driver
            .plan_capture(
                &replacement_binding,
                "cp_fixture_offline_replacement",
                "op_fixture_offline_replacement",
            )
            .unwrap();
        let replacement_version = driver.execute_capture(&replacement_capture).unwrap();
        let replacement_anchor =
            aw_agent_core::SourceAnchor::try_from(&replacement_version).unwrap();
        let replacement_location = GitSourceLocation {
            name: location.name.clone(),
            repository: replacement_repo.clone(),
            repository_identity: DirectoryIdentity::open(&replacement_repo)
                .unwrap()
                .identity()
                .unwrap(),
        };
        let replacement_config = |snapshot: &aw_store::SandboxSnapshot| {
            json!({
                "version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
                "store_generation":store.local_process_info().unwrap().store_generation,
                "manager":actor,"sandbox":snapshot.canonical_ref,"sandbox_rev":snapshot.rev,
                "anchor":replacement_anchor,"destination_root":snapshot.tool_root,
                "destination_identity":DirectoryIdentity::open(&snapshot.tool_root).unwrap().identity().unwrap(),
                "repositories":[replacement_location],"replaced_repository":location
            })
        };
        let config = replacement_config(&attached);
        let gap_config = replacement_config(&attached_recovered);
        let replacement_path = approvals.join("replace.json");
        let gap_path = approvals.join("replace-gap.json");
        for (path, value) in [(&replacement_path, &config), (&gap_path, &gap_config)] {
            let mut file = aw_platform::open_private_new(path).unwrap();
            file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
        }
        let replace = |store: &SessionStore, path: &Path, sandbox: &str, id: &str, mode| {
            crate::sandbox_source_attach::replace(store, &actor, path, sandbox, id, mode)
        };
        let abandon_replacement = |store: &SessionStore, path: &Path, sandbox: &str, id: &str| {
            crate::sandbox_source_attach::abandon_replacement(store, &actor, path, sandbox, id)
        };
        let old_file = attach_root.join("code/tracked");
        fs::write(&old_file, b"independent edit").unwrap();
        assert!(replace(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace",
            Fresh
        )
        .is_err());
        assert_eq!(fs::read(&old_file).unwrap(), b"independent edit");
        fs::write(&old_file, b"dirty checkpoint").unwrap();
        let mut wrong = config.clone();
        wrong["replaced_repository"] = json!(replacement_location);
        fs::write(&replacement_path, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(replace(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace",
            Fresh
        )
        .is_err());
        fs::write(&replacement_path, serde_json::to_vec(&config).unwrap()).unwrap();
        assert!(attach(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace",
            false
        )
        .is_err());
        let interrupt_replacement = |store: &SessionStore,
                                     snapshot: &aw_store::SandboxSnapshot,
                                     id: &str,
                                     release: bool| {
            let prepared = store
                .sandbox_prepare_replacement(
                    &actor,
                    &snapshot.canonical_ref,
                    snapshot.rev,
                    id,
                    replacement_anchor.clone(),
                    |attachment, prepare_id| {
                        let (old, descriptor) = attachment.replaced().unwrap();
                        let target = snapshot.tool_root.join("code");
                        let parent_identity =
                            DirectoryIdentity::open(&snapshot.tool_root)?.identity()?;
                        driver.plan_restore_replacing_approved(
                            &GitRestoreLocation {
                                name: location.name.clone(),
                                repository: replacement_repo.clone(),
                                repository_identity: replacement_location.repository_identity,
                                target: target.clone(),
                                parent_identity,
                            },
                            &replacement_version,
                            prepare_id,
                            &GitRestoreLocation {
                                name: location.name.clone(),
                                repository: repo.clone(),
                                repository_identity: location.repository_identity,
                                target,
                                parent_identity,
                            },
                            &descriptor.prepared,
                            &old.binding,
                        )
                    },
                )
                .unwrap();
            if release {
                prepared.revalidate().unwrap();
                let (old, descriptor) = prepared.observation().replaced().unwrap();
                driver
                    .verify_replacement_untouched(
                        &descriptor.prepared,
                        &old.binding,
                        prepared.source(),
                    )
                    .unwrap();
                driver
                    .release_restored_materialization(&descriptor.prepared, &old.binding)
                    .unwrap();
                driver.require_restore_absent(prepared.source()).unwrap();
            }
            prepared.mark_unknown().unwrap();
        };
        interrupt_replacement(&store, &attached, "fixture:replace-abandon", false);
        let mut permitted = config.clone();
        permitted["allow_continuation"] = json!(true);
        fs::write(&replacement_path, serde_json::to_vec(&permitted).unwrap()).unwrap();
        assert!(replace(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace-abandon",
            Continue
        )
        .is_err());
        assert_eq!(fs::read(&old_file).unwrap(), b"dirty checkpoint");
        abandon_replacement(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace-abandon",
        )
        .unwrap();
        fs::write(&replacement_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let replaced = replace(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace",
            Fresh,
        )
        .unwrap();
        assert_eq!(replaced.rev, attached.rev + 1);
        assert_eq!(replaced.sources[0].binding_rev, replaced.rev);
        assert_eq!(replaced.layout_digest, attached.layout_digest);
        assert_eq!(fs::read(&old_file).unwrap(), b"replacement version");
        abandon_replacement(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace-abandon",
        )
        .unwrap();
        assert!(abandon_replacement(
            &store,
            &replacement_path,
            &attached.canonical_ref,
            "fixture:replace"
        )
        .is_err());

        // Stop after exact predecessor release. Inspect-only recovery and
        // abandonment cannot recreate or discard the old logical row.
        interrupt_replacement(&store, &attached_recovered, "fixture:replace-gap", true);
        assert!(!attach_pending_root.join("code").exists());
        assert!(replace(
            &store,
            &gap_path,
            &pending_empty.canonical_ref,
            "fixture:replace-gap",
            Recover
        )
        .is_err());
        assert!(abandon_replacement(
            &store,
            &gap_path,
            &pending_empty.canonical_ref,
            "fixture:replace-gap"
        )
        .is_err());
        assert!(replace(
            &store,
            &gap_path,
            &pending_empty.canonical_ref,
            "fixture:replace-gap",
            Continue
        )
        .is_err());
        assert_eq!(
            store
                .sandbox_get(&actor, &pending_empty.canonical_ref)
                .unwrap(),
            attached_recovered
        );
        assert!(store
            .sandbox_close(
                &actor,
                &pending_empty.canonical_ref,
                Some(attached_recovered.rev)
            )
            .is_err());
        let mut continuation = gap_config.clone();
        continuation["allow_continuation"] = json!(true);
        let mut wrong = continuation.clone();
        wrong["sandbox_rev"] = json!(attached_recovered.rev + 1);
        fs::write(&gap_path, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(replace(
            &store,
            &gap_path,
            &pending_empty.canonical_ref,
            "fixture:replace-gap",
            Continue
        )
        .is_err());
        assert!(store
            .sandbox_recover_attachment_exact(
                &actor,
                &pending_empty.canonical_ref,
                "fixture:replace-gap",
                SandboxMaterializationRequest {
                    expected_rev: attached_recovered.rev,
                    anchor,
                    replace: true
                }
            )
            .is_err());
        fs::write(&gap_path, serde_json::to_vec(&continuation).unwrap()).unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let gap_replaced = replace(
            &store,
            &gap_path,
            &pending_empty.canonical_ref,
            "fixture:replace-gap",
            Continue,
        )
        .unwrap();
        assert_eq!(gap_replaced.rev, attached_recovered.rev + 1);
        assert_eq!(
            fs::read(attach_pending_root.join("code/tracked")).unwrap(),
            b"replacement version"
        );
        assert_eq!(
            replace(
                &store,
                &gap_path,
                &pending_empty.canonical_ref,
                "fixture:replace-gap",
                Continue
            )
            .unwrap(),
            gap_replaced
        );
        assert_eq!(
            store.agent_list(&actor, project, None, 64).unwrap(),
            agents_before
        );
        assert_eq!(
            store
                .checkpoint_list_metadata(&actor, agent_ref, None, 64)
                .unwrap(),
            checkpoints_before
        );
        assert_eq!(
            git(
                &executable,
                &repo,
                &["for-each-ref", "refs/aw/checkpoints/"]
            ),
            retained_refs
        );
        assert_eq!(git(&executable, &repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(repo.join("tracked")).unwrap(), b"dirty checkpoint");
        assert_eq!(
            git(
                &executable,
                &replacement_repo,
                &["rev-parse", &replacement_version.retention_ref()]
            ),
            replacement_version.captured_commit
        );
        // Exact named release of the original Applied replacement. No
        // metadata-only removal can turn retained custody into an ambient root.
        let release_path = approvals.join("source-release.json");
        let release_config = json!({
            "version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor,"sandbox":replaced.canonical_ref,"sandbox_rev":replaced.rev,
            "source":location.name,"destination_root":attach_root,
            "destination_identity":DirectoryIdentity::open(&attach_root).unwrap().identity().unwrap(),
            "repositories":[replacement_location]
        });
        let mut file = aw_platform::open_private_new(&release_path).unwrap();
        file.write_all(&serde_json::to_vec(&release_config).unwrap())
            .unwrap();
        drop(file);
        let release = |store: &SessionStore| {
            crate::sandbox_retention::release(
                store,
                &actor,
                &release_path,
                &replaced.canonical_ref,
                &location.name,
            )
        };
        assert!(crate::sandbox_retention::Approval::open(&release_path, &actor).is_err());
        assert!(crate::sandbox_retention::release(
            &store,
            &actor,
            &sandbox_path,
            &replaced.canonical_ref,
            &location.name
        )
        .is_err());
        // A replacement's old release provenance is not a removal receipt.
        assert!(store
            .sandbox_replay_source_release(
                &actor,
                &replaced.canonical_ref,
                &location.name,
                attached.rev
            )
            .is_err());
        assert!(store
            .sandbox_replay_source_release(
                &actor,
                &replaced.canonical_ref,
                &location.name,
                replaced.rev
            )
            .unwrap()
            .is_none());
        let mut wrong = release_config.clone();
        wrong["repositories"][0] = json!(location);
        fs::write(&release_path, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(release(&store).is_err());
        fs::write(&release_path, serde_json::to_vec(&release_config).unwrap()).unwrap();
        let foreign = attach_root.join("foreign");
        fs::write(&foreign, b"keep").unwrap();
        assert!(release(&store).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        fs::remove_file(foreign).unwrap();
        let old_file = attach_root.join("code/tracked");
        fs::write(&old_file, b"independent edit").unwrap();
        assert!(release(&store).is_err());
        assert_eq!(fs::read(&old_file).unwrap(), b"independent edit");
        assert_eq!(
            store.sandbox_get(&actor, &replaced.canonical_ref).unwrap(),
            replaced
        );
        assert!(store
            .create(&actor, "unresolved release", layout(&attach_root))
            .is_err());
        assert!(store
            .sandbox_close(&actor, &replaced.canonical_ref, Some(replaced.rev))
            .is_err());
        fs::write(&old_file, b"replacement version").unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let source_free = release(&store).unwrap();
        assert_eq!(source_free.rev, replaced.rev + 1);
        assert!(source_free.sources.is_empty());
        assert_eq!(source_free.layout_digest, replaced.layout_digest);
        assert!(attach_root.is_dir());
        assert!(fs::read_dir(&attach_root).unwrap().next().is_none());
        assert_eq!(release(&store).unwrap(), source_free);
        assert!(store
            .sandbox_replay_source_release(
                &actor,
                &replaced.canonical_ref,
                &SourceName::new("other").unwrap(),
                replaced.rev
            )
            .is_err());
        assert!(store
            .sandbox_replay_source_release(
                &actor,
                &replaced.canonical_ref,
                &location.name,
                replaced.rev - 1
            )
            .is_err());
        // Reattachment at the same name and pathname has distinct original
        // custody. Replaying the earlier release must not inspect or remove it.
        let reattach_path = approvals.join("reattach.json");
        let reattach_config = json!({
            "version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor,"sandbox":source_free.canonical_ref,"sandbox_rev":source_free.rev,
            "anchor":anchor,"destination_root":attach_root,
            "destination_identity":DirectoryIdentity::open(&attach_root).unwrap().identity().unwrap(),
            "repositories":[location]
        });
        let mut file = aw_platform::open_private_new(&reattach_path).unwrap();
        file.write_all(&serde_json::to_vec(&reattach_config).unwrap())
            .unwrap();
        drop(file);
        let reattached = attach(
            &store,
            &reattach_path,
            &source_free.canonical_ref,
            "fixture:reattach-after-release",
            false,
        )
        .unwrap();
        assert_eq!(reattached.rev, source_free.rev + 1);
        assert_eq!(reattached.sources[0].binding_rev, reattached.rev);
        assert_eq!(release(&store).unwrap(), reattached);
        assert_eq!(
            fs::read(attach_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert_eq!(
            fs::read(attach_pending_root.join("code/tracked")).unwrap(),
            b"replacement version"
        );
        assert_eq!(
            git(
                &executable,
                &replacement_repo,
                &["rev-parse", &replacement_version.retention_ref()]
            ),
            replacement_version.captured_commit
        );
        assert_eq!(
            store.agent_list(&actor, project, None, 64).unwrap(),
            agents_before
        );
        assert_eq!(
            store
                .checkpoint_list_metadata(&actor, agent_ref, None, 64)
                .unwrap(),
            checkpoints_before
        );
        // The same configured replacement owner may now operate on the
        // restored, exclusively bound held Agent. Keep the original Agent,
        // its history, exact grant evidence and queue independent of the
        // Sandbox mutation. Qualify an actual post-release interruption.
        let bound_ref = &recovered.agent.canonical_ref;
        let bound_agent = store.agent_get(&actor, bound_ref).unwrap();
        let bound = store
            .sandbox_get(&actor, bound_agent.sandbox_ref.as_deref().unwrap())
            .unwrap();
        assert_eq!(bound_agent.admission, AgentAdmission::Held);
        let bound_execution = store.agent_execution_binding(&actor, bound_ref).unwrap();
        let queue_before = store
            .actor_queue_list(&actor, bound_execution.session_id())
            .unwrap();
        let (_, selected) = store
            .sandbox_grant_sources(
                &actor,
                &actor,
                bound_ref,
                bound.rev,
                std::slice::from_ref(&location.name),
            )
            .unwrap();
        let policy = aw_store::SandboxGrantPolicy {
            agent_ref: bound_ref.clone(),
            sandbox_ref: bound.canonical_ref.clone(),
            evidence: aw_store::SandboxPolicyEvidence {
                policy_ref: "host:bound-replacement".into(),
                policy_digest: [71; 32],
                authority_ref: "host:bound-replacement-approval".into(),
                authority_generation: 1,
            },
            actor: actor.clone(),
            actor_user_id: store.current_user(&actor).unwrap().user_id,
            sources: vec![aw_store::SandboxSourceGrant::from_binding(
                &selected[0],
                SourceAccess::ReadWrite,
            )],
        };
        let ceilings = [aw_agent_core::NamedSourceAccess {
            name: location.name.clone(),
            access: SourceAccess::ReadWrite,
        }];
        let grant = store
            .sandbox_grant_set(&actor, bound_ref, 0, Some(&policy))
            .unwrap();
        assert!(store
            .sandbox_grant_preflight(&actor, bound_ref, &policy, &ceilings)
            .is_ok());
        let bound_path = approvals.join("replace-held-agent.json");
        let mut bound_config = config.clone();
        bound_config["sandbox"] = json!(bound.canonical_ref);
        bound_config["sandbox_rev"] = json!(bound.rev);
        bound_config["destination_root"] = json!(bound.tool_root);
        bound_config["destination_identity"] = json!(DirectoryIdentity::open(&bound.tool_root)
            .unwrap()
            .identity()
            .unwrap());
        bound_config["allow_continuation"] = json!(true);
        aw_platform::open_private_new(&bound_path)
            .unwrap()
            .write_all(&serde_json::to_vec(&bound_config).unwrap())
            .unwrap();
        let bound_id = "fixture:replace-held-agent";
        let bound_set = SourceMutationSet::from_bindings(&[selected[0].binding.clone()]).unwrap();
        let active = store
            .acquire_source_mutation_barrier(&bound_set, "fixture:bound-active-source")
            .unwrap();
        assert!(replace(&store, &bound_path, &bound.canonical_ref, bound_id, Fresh).is_err());
        assert_eq!(
            fs::read(bound.tool_root.join("code/tracked")).unwrap(),
            b"dirty checkpoint"
        );
        active.settle(SourceMutationOutcome::Settled).unwrap();
        assert!(matches!(
            store.sandbox_prepare_attachment(
                &actor,
                &bound.canonical_ref,
                bound.rev,
                "fixture:bound-attach-occupied",
                replacement_anchor.clone(),
                |_, _| panic!("bound attachment reached planner"),
            ),
            Err(Error::Conflict(
                "source materialization requires the selected name to match its attach/replace mode"
            ))
        ));
        assert!(matches!(
            store.sandbox_release_source(
                &actor,
                &bound.canonical_ref,
                &location.name,
                bound.rev,
                |_| -> Result<()> { Err(Error::Denied("fixture: no final-release approval")) },
                |_, _| panic!("unapproved final release reached physical owner"),
            ),
            Err(Error::Denied("fixture: no final-release approval"))
        ));
        interrupt_replacement(&store, &bound, bound_id, true);
        assert!(!bound.tool_root.join("code").exists());
        assert_eq!(
            store.sandbox_get(&actor, &bound.canonical_ref).unwrap(),
            bound
        );
        assert!(replace(&store, &bound_path, &bound.canonical_ref, bound_id, Fresh).is_err());
        assert!(store
            .acquire_source_mutation_barrier(&bound_set, "fixture:bound-gap")
            .is_err());
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let bound_replaced = replace(
            &store,
            &bound_path,
            &bound.canonical_ref,
            bound_id,
            Continue,
        )
        .unwrap();
        assert_eq!(bound_replaced.rev, bound.rev + 1);
        assert_eq!(bound_replaced.sources[0].binding_rev, bound_replaced.rev);
        assert_eq!(bound_replaced.layout_digest, bound.layout_digest);
        assert_eq!(
            fs::read(bound.tool_root.join("code/tracked")).unwrap(),
            b"replacement version"
        );
        assert_eq!(store.agent_get(&actor, bound_ref).unwrap(), bound_agent);
        assert_eq!(
            store
                .actor_queue_list(&actor, bound_execution.session_id())
                .unwrap(),
            queue_before
        );
        assert_eq!(
            store.sandbox_grant_status(&actor, bound_ref).unwrap(),
            grant
        );
        assert!(store
            .sandbox_grant_preflight(&actor, bound_ref, &policy, &ceilings)
            .is_err());
        assert!(store
            .sandbox_grant_set(&actor, bound_ref, grant.revision, Some(&policy))
            .is_err());
        assert!(store
            .agent_admission(
                &actor,
                bound_ref,
                AgentAdmission::Open,
                Some(bound_agent.rev)
            )
            .is_err());
        let (_, current) = store
            .sandbox_grant_sources(
                &actor,
                &actor,
                bound_ref,
                bound_replaced.rev,
                std::slice::from_ref(&location.name),
            )
            .unwrap();
        let renewed = aw_store::SandboxGrantPolicy {
            evidence: aw_store::SandboxPolicyEvidence {
                authority_generation: 2,
                ..policy.evidence.clone()
            },
            sources: vec![aw_store::SandboxSourceGrant::from_binding(
                &current[0],
                SourceAccess::ReadWrite,
            )],
            ..policy
        };
        assert_eq!(
            store
                .sandbox_grant_set(&actor, bound_ref, grant.revision, Some(&renewed))
                .unwrap()
                .revision,
            grant.revision + 1
        );
        assert!(store
            .sandbox_grant_preflight(&actor, bound_ref, &renewed, &ceilings)
            .is_ok());
        assert_eq!(store.agent_get(&actor, bound_ref).unwrap(), bound_agent);
        assert_eq!(
            replace(
                &store,
                &bound_path,
                &bound.canonical_ref,
                bound_id,
                Continue
            )
            .unwrap(),
            bound_replaced
        );
        // Add a second independently retained source to this same held Agent,
        // then remove it using the existing configured source setter. Neither
        // operation may consume Pending work or broaden the earlier grant.
        let writer_epoch = store
            .workspace_writer(
                &actor,
                None,
                &store
                    .session(&actor, bound_execution.session_id())
                    .unwrap()
                    .workspace_id,
            )
            .unwrap()
            .writer_epoch;
        let pending = store
            .actor_queue_enqueue(
                &actor,
                aw_store::ActorQueueRequest {
                    request_id: "fixture-bound-source-mutation".into(),
                    session_id: bound_execution.session_id().to_owned(),
                    branch_id: bound_execution.branch_id().to_owned(),
                    recipient_site: "local".into(),
                    sender: "fixture-bound".into(),
                    message_id: 1,
                    writer_epoch,
                    target: aw_store::QueueTarget::Auto,
                    priority: 0,
                },
                "Keep this original Pending input",
            )
            .unwrap();
        let bound_agent = store.agent_get(&actor, bound_ref).unwrap();
        let queue_before = vec![pending];
        assert_eq!(bound_agent.admission, AgentAdmission::Held);
        let data_name = SourceName::new("data").unwrap();
        let data_binding = driver
            .binding(
                &repo,
                data_name.clone(),
                SourceAccess::ReadWrite,
                CheckpointPolicy::Required,
            )
            .unwrap();
        let data_capture = driver
            .plan_capture(
                &data_binding,
                "cp_fixture_bound_attachment",
                "op_fixture_bound_attachment",
            )
            .unwrap();
        let data_version = driver.execute_capture(&data_capture).unwrap();
        let data_anchor = aw_agent_core::SourceAnchor::try_from(&data_version).unwrap();
        let data_location = GitSourceLocation {
            name: data_name.clone(),
            repository: repo.clone(),
            repository_identity: location.repository_identity,
        };
        let target = bound.tool_root.join("data");
        let parent_identity = DirectoryIdentity::open(&bound.tool_root)
            .unwrap()
            .identity()
            .unwrap();
        let setter_path = approvals.join("bound-source-set.json");
        let setter_config = json!({
            "version":1,"store_directory":state,"store_id":store.store_id().unwrap(),
            "store_generation":store.local_process_info().unwrap().store_generation,
            "manager":actor,"sandbox":bound.canonical_ref,"sandbox_rev":bound_replaced.rev,
            "anchor":data_anchor,"destination_root":bound.tool_root,
            "destination_identity":parent_identity,"repositories":[replacement_location,data_location]
        });
        aw_platform::open_private_new(&setter_path)
            .unwrap()
            .write_all(&serde_json::to_vec(&setter_config).unwrap())
            .unwrap();
        let apply = |store: &SessionStore, id: &str, params: &serde_json::Value| {
            crate::sandbox_source_rpc::ApprovalFile::open(
                &setter_path,
                store,
                &actor,
                project,
                &layout(&initial),
            )?
            .apply(store, &actor, project, id, params.clone())
        };
        let add_params = json!({
            "ref":bound.canonical_ref,"rev":bound_replaced.rev,"name":data_name,
            "binding":{"source":{"driver":data_anchor.driver,"source_ref":data_anchor.source_ref},
                "checkpoint":"required","access":"rw"}
        });
        let add_id = "fixture-bound-attach";
        let incoming_set =
            SourceMutationSet::from_bindings(std::slice::from_ref(&data_binding)).unwrap();
        let busy = store
            .acquire_source_mutation_barrier(&incoming_set, "fixture:incoming-busy")
            .unwrap();
        assert!(apply(&store, add_id, &add_params).is_err());
        assert!(!target.exists());
        busy.settle(SourceMutationOutcome::Settled).unwrap();
        // Lose the original owner after its real incoming effect, before
        // publication. Recover precisely that RPC invocation, never replan.
        let invocation = crate::sandbox_source_attach::rpc_invocation_id(add_id).unwrap();
        let prepared = store
            .sandbox_prepare_attachment(
                &actor,
                &bound.canonical_ref,
                bound_replaced.rev,
                &invocation,
                data_anchor.clone(),
                |_, prepare_id| {
                    driver.plan_restore_approved(
                        &GitRestoreLocation {
                            name: data_name.clone(),
                            repository: repo.clone(),
                            repository_identity: data_location.repository_identity,
                            target: target.clone(),
                            parent_identity,
                        },
                        &data_version,
                        prepare_id,
                    )
                },
            )
            .unwrap();
        prepared.revalidate().unwrap();
        driver.execute_restore(prepared.source()).unwrap();
        prepared.mark_unknown().unwrap();
        assert_eq!(
            store.sandbox_get(&actor, &bound.canonical_ref).unwrap(),
            bound_replaced
        );
        assert!(apply(&store, add_id, &add_params).is_err());
        assert!(store
            .acquire_source_mutation_barrier(&incoming_set, "fixture:add-gap")
            .is_err());
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let added = attach(
            &store,
            &setter_path,
            &bound.canonical_ref,
            &invocation,
            true,
        )
        .unwrap();
        assert_eq!(added.rev, bound_replaced.rev + 1);
        assert_eq!(added.sources[0], bound_replaced.sources[0]);
        assert_eq!(added.sources[1].binding_rev, added.rev);
        assert_eq!(added.layout_digest, bound.layout_digest);
        assert_eq!(apply(&store, add_id, &add_params).unwrap(), added);
        assert_eq!(
            fs::read(target.join("tracked")).unwrap(),
            b"dirty checkpoint"
        );
        let mut both_ceilings = ceilings.to_vec();
        both_ceilings.push(aw_agent_core::NamedSourceAccess {
            name: data_name.clone(),
            access: SourceAccess::ReadWrite,
        });
        assert_eq!(
            store
                .sandbox_grant_preflight(&actor, bound_ref, &renewed, &both_ceilings)
                .unwrap()
                .sources
                .len(),
            1,
        );
        let (_, all_bindings) = store
            .sandbox_grant_sources(
                &actor,
                &actor,
                bound_ref,
                added.rev,
                &[location.name.clone(), data_name.clone()],
            )
            .unwrap();
        let expanded = aw_store::SandboxGrantPolicy {
            evidence: aw_store::SandboxPolicyEvidence {
                authority_generation: 3,
                ..renewed.evidence.clone()
            },
            sources: all_bindings
                .iter()
                .map(|source| {
                    aw_store::SandboxSourceGrant::from_binding(source, SourceAccess::ReadWrite)
                })
                .collect(),
            ..renewed.clone()
        };
        let expanded_grant = store
            .sandbox_grant_set(&actor, bound_ref, grant.revision + 1, Some(&expanded))
            .unwrap();
        assert_eq!(
            store
                .sandbox_grant_preflight(&actor, bound_ref, &expanded, &both_ceilings)
                .unwrap()
                .sources
                .len(),
            2
        );
        assert_eq!(
            store
                .actor_queue_list(&actor, bound_execution.session_id())
                .unwrap(),
            queue_before
        );

        // Release only the selected original managed checkout. A modified
        // selected checkout stays held in Unknown custody; sibling edits
        // remain intact through the exact original release on restart.
        let mut remove_config = setter_config.clone();
        let fields = remove_config.as_object_mut().unwrap();
        fields.remove("anchor");
        fields.insert("sandbox_rev".into(), json!(added.rev));
        fields.insert("source".into(), json!(data_name));
        fs::write(&setter_path, serde_json::to_vec(&remove_config).unwrap()).unwrap();
        let remove_params =
            json!({"ref":bound.canonical_ref,"rev":added.rev,"name":data_name,"binding":null});
        let retained_file = bound.tool_root.join("code/tracked");
        fs::write(&retained_file, b"retained sibling edit").unwrap();
        let data_file = target.join("tracked");
        fs::write(&data_file, b"independent edit").unwrap();
        assert!(apply(&store, "fixture-bound-release", &remove_params).is_err());
        assert_eq!(fs::read(&data_file).unwrap(), b"independent edit");
        assert_eq!(
            store.sandbox_get(&actor, &bound.canonical_ref).unwrap(),
            added
        );
        assert!(store
            .acquire_source_mutation_barrier(
                &SourceMutationSet::from_bindings(&[all_bindings[0].binding.clone()]).unwrap(),
                "fixture:partial-release-gap",
            )
            .is_err());
        fs::write(&data_file, b"dirty checkpoint").unwrap();
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        let reduced = apply(&store, "fixture-bound-release-reconcile", &remove_params).unwrap();
        assert_eq!(reduced.rev, added.rev + 1);
        assert_eq!(reduced.sources, bound_replaced.sources);
        assert_eq!(reduced.layout_digest, bound.layout_digest);
        assert!(!target.exists());
        assert_eq!(fs::read(&retained_file).unwrap(), b"retained sibling edit");
        assert_eq!(
            apply(&store, "fixture-bound-release-replay", &remove_params).unwrap(),
            reduced
        );
        assert_eq!(
            store.sandbox_grant_status(&actor, bound_ref).unwrap(),
            expanded_grant
        );
        assert!(store
            .sandbox_grant_preflight(&actor, bound_ref, &expanded, &both_ceilings)
            .is_err());
        assert_eq!(store.agent_get(&actor, bound_ref).unwrap(), bound_agent);
        assert_eq!(
            store
                .actor_queue_list(&actor, bound_execution.session_id())
                .unwrap(),
            queue_before
        );
        assert!(store
            .agent_admission(
                &actor,
                bound_ref,
                AgentAdmission::Open,
                Some(bound_agent.rev)
            )
            .is_err());
        // The exact original final removal leaves an empty *source-only*
        // Sandbox, not a new ambient execution capability. Restore the
        // selected checkout's original contents before approving release.
        fs::write(&retained_file, b"replacement version").unwrap();
        let mut final_config = remove_config.clone();
        final_config["sandbox_rev"] = json!(reduced.rev);
        final_config["source"] = json!(location.name);
        final_config["repositories"] = json!([replacement_location]);
        let final_params = json!({
            "ref":bound.canonical_ref,"rev":reduced.rev,"name":location.name,"binding":null
        });
        fs::write(&setter_path, serde_json::to_vec(&final_config).unwrap()).unwrap();
        let blocked = store
            .acquire_source_mutation_barrier(
                &SourceMutationSet::from_bindings(&[all_bindings[0].binding.clone()]).unwrap(),
                "fixture:bound-final-release-busy",
            )
            .unwrap();
        assert!(apply(&store, "fixture-bound-final-release", &final_params).is_err());
        blocked.settle(SourceMutationOutcome::Settled).unwrap();
        let held_empty = apply(&store, "fixture-bound-final-release", &final_params).unwrap();
        assert!(held_empty.sources.is_empty());
        assert_eq!(held_empty.rev, reduced.rev + 1);
        assert_eq!(held_empty.layout_digest, reduced.layout_digest);
        assert_eq!(fs::read_dir(&bound.tool_root).unwrap().count(), 0);
        let assert_held_empty = |store: &SessionStore| {
            assert_eq!(
                store.sandbox_get(&actor, &bound.canonical_ref).unwrap(),
                held_empty
            );
            assert_eq!(store.agent_get(&actor, bound_ref).unwrap(), bound_agent);
            assert_eq!(
                store
                    .actor_queue_list(&actor, bound_execution.session_id())
                    .unwrap(),
                queue_before
            );
            assert!(matches!(
                store.agent_admission(
                    &actor,
                    bound_ref,
                    AgentAdmission::Open,
                    Some(bound_agent.rev)
                ),
                Err(Error::Denied(
                    "source-bearing Agent Sandbox realization is unavailable"
                ))
            ));
            assert!(matches!(
                store.activate(
                    &actor,
                    bound_execution.session_id(),
                    bound_execution.branch_id()
                ),
                Err(Error::Denied(
                    "source-bearing Agent Sandbox realization is unavailable"
                ))
            ));
            // A new Session cannot select the protected empty layout
            // through the automatic legacy binding path, either.
            assert!(store
                .create(
                    &actor,
                    "fixture:empty-source-alias",
                    layout(&bound.tool_root)
                )
                .is_err());
            assert!(store
                .sandbox_grant_preflight(&actor, bound_ref, &expanded, &both_ceilings)
                .is_err());
        };
        assert_held_empty(&store);
        drop(store);
        let store =
            SessionStore::open_with_sources(&state, registry.clone(), Default::default()).unwrap();
        assert_held_empty(&store);
        assert_eq!(
            apply(&store, "fixture-bound-final-replay", &final_params).unwrap(),
            held_empty
        );

        // The same original first-source layout reservation now supports
        // reattachment to this protected empty bound layout. The ordinary
        // ambient gate is never released, even between physical effect
        // and original receipt settlement or when that receipt is Unknown.
        let mut readd_config = setter_config;
        readd_config["sandbox_rev"] = json!(held_empty.rev);
        readd_config["repositories"] = json!([data_location]);
        let mut readd_params = add_params;
        readd_params["rev"] = json!(held_empty.rev);
        fs::write(&setter_path, serde_json::to_vec(&readd_config).unwrap()).unwrap();
        let readd_id =
            crate::sandbox_source_attach::rpc_invocation_id("fixture-bound-reattach").unwrap();
        let prepared = store
            .sandbox_prepare_attachment(
                &actor,
                &bound.canonical_ref,
                held_empty.rev,
                &readd_id,
                data_anchor.clone(),
                |_, prepare_id| {
                    driver.plan_restore_approved(
                        &GitRestoreLocation {
                            name: data_name.clone(),
                            repository: repo.clone(),
                            repository_identity: data_location.repository_identity,
                            target: target.clone(),
                            parent_identity,
                        },
                        &data_version,
                        prepare_id,
                    )
                },
            )
            .unwrap();
        prepared.revalidate().unwrap();
        driver.execute_restore(prepared.source()).unwrap();
        prepared.mark_unknown().unwrap();
        assert_held_empty(&store);
        assert!(apply(&store, "fixture-bound-reattach", &readd_params).is_err());
        assert!(store
            .sandbox_close(&actor, &bound.canonical_ref, None)
            .is_err());
        drop(store);
        let store = SessionStore::open_with_sources(&state, registry, Default::default()).unwrap();
        assert_held_empty(&store);
        let readded = attach(&store, &setter_path, &bound.canonical_ref, &readd_id, true).unwrap();
        assert_eq!(readded.rev, held_empty.rev + 1);
        assert_eq!(readded.sources.len(), 1);
        assert_eq!(readded.sources[0].name, data_name);
        assert_eq!(readded.sources[0].binding_rev, readded.rev);
        assert_eq!(
            apply(&store, "fixture-bound-reattach", &readd_params).unwrap(),
            readded
        );
        fs::write(&setter_path, serde_json::to_vec(&final_config).unwrap()).unwrap();
        assert_eq!(
            apply(&store, "fixture-bound-final-old-replay", &final_params).unwrap(),
            readded
        );
        fs::write(&setter_path, serde_json::to_vec(&remove_config).unwrap()).unwrap();
        assert_eq!(
            apply(&store, "fixture-bound-release-old-replay", &remove_params).unwrap(),
            readded
        );
        assert_eq!(
            fs::read(target.join("tracked")).unwrap(),
            b"dirty checkpoint"
        );
        assert!(store
            .sandbox_grant_preflight(&actor, bound_ref, &expanded, &both_ceilings)
            .is_err());
        assert_eq!(store.agent_get(&actor, bound_ref).unwrap(), bound_agent);
        assert_eq!(
            store
                .actor_queue_list(&actor, bound_execution.session_id())
                .unwrap(),
            queue_before
        );
        drop(store);
        // Terminal replay uses the tombstone even without installed Git or the checkout.
        fs::rename(&replacement_repo, root.join("moved-replacement-origin")).unwrap();
        fs::rename(&repo, root.join("moved-repo")).unwrap();
        fs::rename(&destination, root.join("released-layout")).unwrap();
        let store = SessionStore::open(&state).unwrap();
        assert_eq!(release(&store).unwrap().rev, reattached.rev);
        assert_eq!(
            replace(
                &store,
                &replacement_path,
                &empty.canonical_ref,
                "fixture:replace",
                Fresh
            )
            .unwrap()
            .rev,
            reattached.rev
        );
        assert_eq!(
            replace(
                &store,
                &gap_path,
                &pending_empty.canonical_ref,
                "fixture:replace-gap",
                Continue
            )
            .unwrap()
            .rev,
            gap_replaced.rev
        );
        abandon_replacement(
            &store,
            &replacement_path,
            &empty.canonical_ref,
            "fixture:replace-abandon",
        )
        .unwrap();
        assert_eq!(
            attach(
                &store,
                &attach_path,
                &empty.canonical_ref,
                "fixture:attach",
                false
            )
            .unwrap()
            .rev,
            reattached.rev
        );
        assert_eq!(
            attach(
                &store,
                &pending_attach_path,
                &pending_empty.canonical_ref,
                "fixture:attach-pending",
                true
            )
            .unwrap()
            .rev,
            gap_replaced.rev
        );
        abandon_attach(
            &store,
            &attach_path,
            &empty.canonical_ref,
            "fixture:attach-abandon",
        )
        .unwrap();
        assert_eq!(
            create_baseline(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:sandbox-create:fixture-baseline-create",
                false
            )
            .unwrap()
            .canonical_ref,
            standalone.canonical_ref
        );
        assert_eq!(
            create_baseline(
                &store,
                &root.join("absent-approval"),
                "fixture:baseline-pending",
                true
            )
            .unwrap()
            .canonical_ref,
            baseline_recovered.canonical_ref
        );
        abandon_baseline(
            &store,
            &root.join("absent-approval"),
            "fixture:baseline-discard",
        )
        .unwrap();
        assert_eq!(
            derive(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:sandbox-create:fixture-derive",
                false
            )
            .unwrap()
            .canonical_ref,
            child.canonical_ref
        );
        assert_eq!(
            derive(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:sandbox-create:fixture-derive-pending",
                true
            )
            .unwrap()
            .canonical_ref,
            derived.canonical_ref
        );
        abandon_derive(
            &store,
            &root.join("absent-approval"),
            "fixture:derive-discard",
        )
        .unwrap();
        // Metadata of the still-live exact immutable anchor remains readable
        // without the driver or physical checkout, unlike a source effect.
        assert_eq!(
            get_checkpoint(&store, true, &fresh.id).unwrap()["sources"][0]["source"],
            anchor_identity
        );
        assert_eq!(
            get_checkpoint(&store, true, &capture.id).unwrap()["sources"],
            json!([])
        );
        metadata_config["policy"]["sources"] = json!([]);
        fs::write(
            &metadata_path,
            serde_json::to_vec(&metadata_config).unwrap(),
        )
        .unwrap();
        assert!(
            get_checkpoint(&store, true, &fresh.id).unwrap()["sources"][0]
                .get("source")
                .is_none()
        );
        assert_eq!(reconcile_sandbox(&store).unwrap(), released);
        abandon_capture(
            &store,
            &root.join("absent-approval"),
            "fixture:abandon-capture",
        )
        .unwrap();
        abandon_restore(
            &store,
            &root.join("absent-approval"),
            "fixture:abandon-restore",
        )
        .unwrap();
        assert!(crate::checkpoint_restore::abandon(
            &store,
            &actor,
            &root.join("absent-approval"),
            &wrong_request,
            "fixture:abandon-restore"
        )
        .is_err());
        assert_eq!(
            invoke_restore(
                &store,
                &root.join("absent-approval"),
                "rpc:stdio:checkpoint-restore:fixture-restore-interrupted",
                true
            )
            .unwrap(),
            aw_store::CheckpointRestoreResult {
                agent: bound_agent,
                source_coverage: recovered.source_coverage,
            }
        );
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
