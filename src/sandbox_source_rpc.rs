//! Authenticated host source setter over the same private approvals and
//! original Prepared/release owners used by offline maintenance. No
//! request-selected filesystem paths, versions or recovery modes.
use crate::{
    git_maintenance::{open_private_approval, overlaps},
    sandbox_retention,
    sandbox_source_attach::{self, MaterializationMode},
};
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{CheckpointPolicy, DriverKey, SourceAccess, SourceName};
use aw_git::source_driver::GitSourceLocation;
use aw_store::{SandboxSnapshot, SessionBinding, SessionStore};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(untagged)]
enum Config {
    Materialize(Box<sandbox_source_attach::Config>),
    Release(Box<sandbox_retention::Config>),
}

enum Approval {
    Materialize(Box<sandbox_source_attach::Approval>),
    Release(Box<sandbox_retention::Approval>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(rename = "ref")]
    reference: String,
    name: SourceName,
    // Explicit null is removal; omission is never removal.
    binding: Value,
    rev: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    source: SourceRef,
    checkpoint: CheckpointPolicy,
    access: Access,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRef {
    driver: DriverKey,
    source_ref: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Access {
    Ro,
    Rw,
}

impl From<Access> for SourceAccess {
    fn from(access: Access) -> Self {
        match access {
            Access::Ro => Self::ReadOnly,
            Access::Rw => Self::ReadWrite,
        }
    }
}

/// One independently chosen private approval file, scoped to the existing
/// authenticated local Project. Cooperating host writers lease the same
/// file when renewing/withdrawing; each request reads it under that lease.
/// Host owns exclusion of other users, aliases, mounts, backing and state
/// roots. This is not an execution grant or general physical containment.
pub(crate) struct ApprovalFile {
    path: PathBuf,
    protected: Vec<PathBuf>,
}

impl ApprovalFile {
    pub(crate) fn open(
        path: &Path,
        store: &SessionStore,
        actor: &PrincipalId,
        project: &str,
        binding: &SessionBinding,
    ) -> Result<Self> {
        let mut protected = Vec::with_capacity(2);
        for root in [&binding.tool_root, &binding.working_directory] {
            let canonical = root.canonicalize()?;
            if !protected.contains(&canonical) {
                protected.push(canonical);
            }
        }
        let file = Self {
            path: path.to_owned(),
            protected,
        };
        file.read(store, actor, project)?;
        Ok(file)
    }

    fn require_layout<'a>(
        &self,
        root: &Path,
        repositories: impl Iterator<Item = &'a GitSourceLocation>,
    ) -> Result<()> {
        if self.protected.iter().any(|path| overlaps(root, path)) {
            return Err(Error::Denied(
                "source mutation overlaps the bound Agent layout",
            ));
        }
        for location in repositories {
            if self
                .protected
                .iter()
                .any(|path| overlaps(&location.repository, path))
            {
                return Err(Error::Denied(
                    "source mutation origin overlaps the bound Agent layout",
                ));
            }
        }
        Ok(())
    }

    fn read(&self, store: &SessionStore, actor: &PrincipalId, project: &str) -> Result<Approval> {
        let parent = self
            .path
            .parent()
            .ok_or(Error::Denied("source approval directory required"))?;
        let (lease, config): (_, Config) = open_private_approval(&self.path)?;
        let identity = aw_platform::DirectoryIdentity::open(parent)?.identity()?;
        for path in &self.protected {
            if overlaps(parent, path)
                || aw_platform::DirectoryIdentity::open(path)?.identity()? == identity
            {
                return Err(Error::Denied(
                    "source approval overlaps the bound Agent layout",
                ));
            }
        }
        let sandbox = match &config {
            Config::Materialize(c) => &c.sandbox,
            Config::Release(c) => &c.sandbox,
        };
        if sandbox.rsplit_once("/s:").map(|(p, _)| p) != Some(project) {
            return Err(Error::Denied("cross-Project source mutation unavailable"));
        }
        // No physical probes of source/destination before original
        // resource authorization. Exact semantic replay is still allowed
        // after the original materialization has disappeared.
        let current = store.sandbox_get(actor, sandbox)?;
        if current.canonical_ref != *sandbox || current.project_ref != project {
            return Err(Error::Denied(
                "source mutation requires an exact Project Sandbox",
            ));
        }
        match config {
            Config::Materialize(c) => {
                self.require_layout(
                    &c.destination_root,
                    c.repositories.iter().chain(c.replaced_repository.iter()),
                )?;
                let reference = c.sandbox.clone();
                let replace = c.replaced_repository.is_some();
                Ok(Approval::Materialize(Box::new(
                    sandbox_source_attach::Approval::from_config(
                        store, actor, &reference, &self.path, replace, lease, *c,
                    )?,
                )))
            }
            Config::Release(c) => {
                self.require_layout(&c.destination_root, c.repositories.iter())?;
                let name = c.source.clone().ok_or(Error::Denied(
                    "closed retention is not a source setter approval",
                ))?;
                let approval = sandbox_retention::Approval::from_config(
                    &self.path,
                    actor,
                    Some(&name),
                    lease,
                    *c,
                )?;
                approval.require_scope(store)?;
                Ok(Approval::Release(Box::new(approval)))
            }
        }
    }

    pub(crate) fn apply(
        &self,
        store: &SessionStore,
        actor: &PrincipalId,
        project: &str,
        request_id: &str,
        params: Value,
    ) -> Result<SandboxSnapshot> {
        let id = sandbox_source_attach::rpc_invocation_id(request_id)?;
        let request: Request = serde_json::from_value(params)
            .map_err(|_| Error::Protocol("invalid sandbox_sources_set parameters"))?;
        request.name.validate()?;
        if request.reference.len() > 2048
            || request.rev == 0
            || request.rev > i64::MAX as u64
            || request.reference.rsplit_once("/s:").map(|(p, _)| p) != Some(project)
        {
            return Err(Error::Denied(
                "source mutation requires exact current Project ref and revision",
            ));
        }
        let protected = self
            .protected
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        match self.read(store, actor, project)? {
            Approval::Materialize(approval) => {
                let c = approval.config();
                let selection: Binding = serde_json::from_value(request.binding)
                    .map_err(|_| Error::Protocol("invalid source binding selection"))?;
                selection.source.driver.validate()?;
                if request.reference != c.sandbox
                    || request.rev != c.sandbox_rev
                    || request.name != c.anchor.name
                    || selection.source.driver != c.anchor.driver
                    || selection.source.source_ref != c.anchor.source_ref
                    || selection.checkpoint != c.anchor.checkpoint_policy
                    || SourceAccess::from(selection.access) != c.anchor.access
                {
                    return Err(Error::Denied(
                        "source request differs from the independent approved selection",
                    ));
                }
                // Fresh or exact Applied replay only. Unknown effects must
                // be reconciled by the original explicit offline owner.
                approval.materialize(store, actor, &id, MaterializationMode::Fresh, &protected)
            }
            Approval::Release(approval) => {
                let c = approval.config();
                if request.reference != c.sandbox
                    || request.rev != c.sandbox_rev
                    || c.source.as_ref() != Some(&request.name)
                    || !request.binding.is_null()
                {
                    return Err(Error::Denied(
                        "source request differs from the independent removal approval",
                    ));
                }
                // The original revision-fenced release owner already
                // distinguishes fresh, unresolved and settled effects.
                approval.release(store, &request.reference, &protected)
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use aw_agent_core::{SourceAnchor, SourceDriverRegistry};
    use aw_git::source_driver::{GitRestoreLocation, GitSourceDriver};
    use aw_store_api::SessionBackend;
    use serde_json::json;
    use std::{fs, io::Write, process::Command, time::Duration};

    #[test]
    fn configured_stdio_source_set_consumes_original_mutation_owners() {
        use crate::{
            config, resources_cli, sessions, ExecutionContext, Host, PersistentSession, Settings,
        };
        use aw::headless::Driver;

        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let temp = Temp(crate::test_support::temp_dir("stdio-sandbox-source-set"));
        let root = temp.0.canonicalize().unwrap();
        let state_dir = root.join("state");
        let workspace = root.join("workspace");
        let repo = root.join("repo");
        let destination = root.join("destination");
        let approvals = root.join("approvals");
        for path in [&workspace, &repo, &destination, &approvals] {
            aw_platform::ensure_private_dir(path).unwrap();
        }
        let executable = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let git = |args: &[&str]| {
            let result = Command::new(&executable)
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "fixture Git failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            String::from_utf8(result.stdout)
                .unwrap()
                .trim_end()
                .to_owned()
        };
        git(&["init", "--initial-branch=main"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "fixture@example.invalid"]);
        fs::write(repo.join("tracked"), b"first").unwrap();
        git(&["add", "tracked"]);
        git(&["commit", "--no-gpg-sign", "-m", "fixture"]);
        let head = git(&["rev-parse", "HEAD"]);
        let index = fs::read(repo.join(".git/index")).unwrap();
        let registry = SourceDriverRegistry::new();
        let handle = GitSourceDriver::new(&executable)
            .unwrap()
            .register(&registry)
            .unwrap();
        let store =
            SessionStore::open_with_sources(&state_dir, registry.clone(), Default::default())
                .unwrap();
        let driver = store.source_driver(&handle).unwrap();
        let name = SourceName::new("code").unwrap();
        let source_binding = driver
            .binding(
                &repo,
                name.clone(),
                SourceAccess::ReadWrite,
                CheckpointPolicy::Required,
            )
            .unwrap();
        // Fixture owns these already-retained versions independently of
        // any Agent's history or source grant.
        let first = driver
            .execute_capture(
                &driver
                    .plan_capture(&source_binding, "cp_stdio_first", "op_stdio_first")
                    .unwrap(),
            )
            .unwrap();
        fs::write(repo.join("tracked"), b"second").unwrap();
        let second = driver
            .execute_capture(
                &driver
                    .plan_capture(&source_binding, "cp_stdio_second", "op_stdio_second")
                    .unwrap(),
            )
            .unwrap();
        let anchor = SourceAnchor::try_from(&first).unwrap();
        let replacement = SourceAnchor::try_from(&second).unwrap();

        let mut profile = config::builtins().remove("llama").unwrap();
        profile.model = Some("fixture".into());
        profile.base_url = "http://127.0.0.1:1/v1".into();
        let settings = Settings {
            profile_name: "fixture".into(),
            profile,
            system_prompt: "fixture".into(),
            request_timeout: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(2),
            max_output_bytes: config::MAX_OUTPUT_BYTES,
        };
        let context = ExecutionContext::capture(&workspace).unwrap();
        let tools = aw::LocalToolOptions::new(workspace.clone(), aw::ToolMode::Off);
        let binding = sessions::binding(&settings, &context, &tools).unwrap();
        let actor = sessions::actor(&context, false).unwrap();
        let (session, branch) = store
            .create(&actor, "source-set fixture", binding.clone())
            .unwrap();
        let info = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let project = format!(
            "/d:{}/i:{}/p:{}",
            info.domain_handle, info.identity_handle, info.project_handle
        );
        let reference = format!("{project}/a:{}", info.agent_handle);
        let before = store.agent_get(&actor, &reference).unwrap();
        let mut layout = binding.clone();
        layout.tool_root = destination.clone();
        layout.working_directory = destination.clone();
        let sandbox = store
            .sandbox_create_fresh(&actor, &project, &layout)
            .unwrap();
        let location = GitSourceLocation {
            name: name.clone(),
            repository: repo.clone(),
            repository_identity: aw_platform::DirectoryIdentity::open(&repo)
                .unwrap()
                .identity()
                .unwrap(),
        };
        let path = approvals.join("source.json");
        let approval = |store: &SessionStore, rev: u64, anchor: &SourceAnchor| {
            json!({
                "version":1,"store_directory":state_dir,"store_id":store.store_id().unwrap(),
                "store_generation":store.local_process_info().unwrap().store_generation,
                "manager":actor,"sandbox":sandbox.canonical_ref,"sandbox_rev":rev,
                "anchor":anchor,"destination_root":destination,
                "destination_identity":aw_platform::DirectoryIdentity::open(&destination).unwrap().identity().unwrap(),
                "repositories":[location]
            })
        };
        let first_config = approval(&store, sandbox.rev, &anchor);
        let mut file = aw_platform::open_private_new(&path).unwrap();
        file.write_all(&serde_json::to_vec(&first_config).unwrap())
            .unwrap();
        drop(file);

        let make_host = |store: &SessionStore| {
            let persistent = PersistentSession::open(
                store.clone(),
                actor.clone(),
                binding.clone(),
                &session.id,
                &branch.id,
                false,
            )
            .unwrap();
            let mut host = Host::new(
                settings.clone(),
                Default::default(),
                Default::default(),
                Default::default(),
                None,
                context.clone(),
                aw::LocalToolOptions::new(workspace.clone(), aw::ToolMode::Off),
                false,
                aw::ToolMode::Off,
                1,
                1,
                1,
                1,
                60,
                None,
                None,
                Some(state_dir.clone()),
                resources_cli::SessionStoreBackendArg::Sqlite,
                Some(persistent),
                #[cfg(feature = "jsonl")]
                None,
                #[cfg(feature = "jsonl")]
                None,
                None,
                None,
                None,
                None,
            );
            host.ensure(false, "Sandbox source setter fixture").unwrap();
            assert!(host.attach_rpc_sandbox_source(&path).is_err());
            host.attach_rpc_actor_owner().unwrap();
            assert!(host.agent.as_ref().unwrap().tool_specs().is_empty());
            host
        };
        let mut host = make_host(&store);
        let request = |rev, anchor: &SourceAnchor| {
            json!({"ref":sandbox.canonical_ref,
            "name":name,"rev":rev,"binding":{"source":{"driver":anchor.driver,"source_ref":anchor.source_ref},
                "checkpoint":"required","access":"rw"}})
        };
        let frame = |id: &str, params: Value| json!({"v":1,"id":id,"method":"sandbox_sources_set","params":params});
        fn serve(host: &mut Host, requests: Vec<Value>) -> Vec<Value> {
            let input = requests
                .into_iter()
                .map(|request| request.to_string() + "\n")
                .collect::<String>();
            let mut output = Vec::new();
            aw::headless::serve_stdio(host, std::io::Cursor::new(input), &mut output, false)
                .unwrap();
            String::from_utf8(output)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
        fn response<'a>(frames: &'a [Value], id: &str) -> &'a Value {
            frames.iter().find(|frame| frame["id"] == id).unwrap()
        }
        let params = request(sandbox.rev, &anchor);
        let disabled = serve(&mut host, vec![frame("disabled", params.clone())]);
        assert_eq!(
            disabled[0]["data"]["capabilities"]["sandbox_sources_set"],
            false
        );
        assert_eq!(response(&disabled, "disabled")["type"], "error");
        assert!(!destination.join("code").exists());
        let mut overlap = first_config.clone();
        overlap["destination_root"] = json!(workspace);
        fs::write(&path, serde_json::to_vec(&overlap).unwrap()).unwrap();
        assert!(host.attach_rpc_sandbox_source(&path).is_err());
        fs::write(&path, serde_json::to_vec(&first_config).unwrap()).unwrap();
        host.attach_rpc_sandbox_source(&path).unwrap();
        assert!(host.attach_rpc_sandbox_source(&path).is_err());
        assert!(host.can_sandbox_sources_set());

        let mut wrong = params.clone();
        wrong["binding"]["source"]["source_ref"] = json!("not-approved");
        let mut nested = params.clone();
        nested["binding"]["source"]["repository"] = json!("/unapproved");
        let mut override_path = params.clone();
        override_path["approval"] = json!("/unapproved");
        let frames = serve(
            &mut host,
            vec![
                frame("wrong", wrong.clone()),
                frame("wrong", params.clone()),
                frame("nested", nested),
                frame("override", override_path),
                frame("attach", params.clone()),
            ],
        );
        assert_eq!(
            frames[0]["data"]["capabilities"]["sandbox_sources_set"],
            true
        );
        for id in ["wrong", "nested", "override"] {
            assert_eq!(response(&frames, id)["type"], "error");
        }
        assert!(
            frames
                .iter()
                .any(|frame| frame["id"] == "wrong"
                    && frame["data"]["code"] == "duplicate_request_id")
        );
        assert_eq!(response(&frames, "attach")["type"], "response");
        let attached = store.sandbox_get(&actor, &sandbox.canonical_ref).unwrap();
        assert_eq!(attached.rev, sandbox.rev + 1);
        assert_eq!(
            fs::read(destination.join("code/tracked")).unwrap(),
            b"first"
        );
        let encoded = response(&frames, "attach").to_string();
        for protected in [
            anchor.source_ref.as_str(),
            anchor.retention.as_str(),
            repo.to_str().unwrap(),
            destination.to_str().unwrap(),
        ] {
            assert!(!encoded.contains(protected));
        }
        assert_eq!(
            serve(&mut host, vec![frame("attach", params.clone())])[1]["type"],
            "response"
        );
        let mut config = approval(&store, attached.rev, &replacement);
        config["replaced_repository"] = json!(location);
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let replace_params = request(attached.rev, &replacement);
        let frames = serve(
            &mut host,
            vec![
                frame("attach", replace_params.clone()),
                frame("replace", replace_params),
            ],
        );
        assert_eq!(response(&frames, "attach")["type"], "error");
        assert_eq!(response(&frames, "replace")["type"], "response");
        let replaced = store.sandbox_get(&actor, &sandbox.canonical_ref).unwrap();
        assert_eq!(replaced.rev, attached.rev + 1);
        assert_eq!(
            fs::read(destination.join("code/tracked")).unwrap(),
            b"second"
        );

        config = approval(&store, replaced.rev, &replacement);
        let object = config.as_object_mut().unwrap();
        object.remove("anchor");
        object.insert("source".into(), json!(name));
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let release_params =
            json!({"ref":sandbox.canonical_ref,"name":name,"binding":null,"rev":replaced.rev});
        let mut missing = release_params.clone();
        missing.as_object_mut().unwrap().remove("binding");
        let frames = serve(
            &mut host,
            vec![
                frame("missing", missing),
                frame("release", release_params.clone()),
            ],
        );
        assert_eq!(response(&frames, "missing")["type"], "error");
        assert_eq!(response(&frames, "release")["type"], "response");
        let released = store.sandbox_get(&actor, &sandbox.canonical_ref).unwrap();
        assert_eq!(released.rev, replaced.rev + 1);
        assert!(released.sources.is_empty());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_eq!(
            serve(&mut host, vec![frame("release", release_params)])[1]["type"],
            "response"
        );

        // Original RPC custody is recoverable by the same explicit offline
        // owner, not by another transport attempt or a new invocation.
        let config = approval(&store, released.rev, &anchor);
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let id = sandbox_source_attach::rpc_invocation_id("interrupted").unwrap();
        let prepared = store
            .sandbox_prepare_attachment(
                &actor,
                &sandbox.canonical_ref,
                released.rev,
                &id,
                anchor.clone(),
                |_, prepare_id| {
                    driver.plan_restore_approved(
                        &GitRestoreLocation {
                            name: name.clone(),
                            repository: repo.clone(),
                            repository_identity: location.repository_identity,
                            target: destination.join("code"),
                            parent_identity: aw_platform::DirectoryIdentity::open(&destination)?
                                .identity()?,
                        },
                        &first,
                        prepare_id,
                    )
                },
            )
            .unwrap();
        prepared.revalidate().unwrap();
        driver.execute_restore(prepared.source()).unwrap();
        prepared.mark_unknown().unwrap();
        let interrupted = request(released.rev, &anchor);
        assert_eq!(
            serve(&mut host, vec![frame("interrupted", interrupted.clone())])[1]["type"],
            "error"
        );
        assert_eq!(
            store.sandbox_get(&actor, &sandbox.canonical_ref).unwrap(),
            released
        );
        drop(host);
        drop(store);
        let store =
            SessionStore::open_with_sources(&state_dir, registry, Default::default()).unwrap();
        let recovered =
            sandbox_source_attach::attach(&store, &actor, &path, &sandbox.canonical_ref, &id, true)
                .unwrap();
        let mut host = make_host(&store);
        host.attach_rpc_sandbox_source(&path).unwrap();
        assert_eq!(
            serve(&mut host, vec![frame("interrupted", interrupted)])[1]["type"],
            "response"
        );
        assert_eq!(
            store.sandbox_get(&actor, &sandbox.canonical_ref).unwrap(),
            recovered
        );
        assert_eq!(store.agent_get(&actor, &reference).unwrap(), before);
        assert!(store
            .actor_queue_list(&actor, &session.id)
            .unwrap()
            .is_empty());
        assert_eq!(git(&["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(repo.join("tracked")).unwrap(), b"second");
    }
}
