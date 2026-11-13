//! The foreground trusted Bash tool is a granted Process effect. Its actual
//! subprocess uses the SAME host process lifecycle/capture service as other
//! pipe-based process actors. No alternate Bash-specific reaper/capture loop.
//! Same-UID trusted shell access is emphatically NOT an OS sandbox.
use crate::process::{Outcome as ProcessOutcome, PipeProcess, PipeSpec};
use aw_agent_core::*;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

#[derive(Clone, Default)]
pub(crate) struct ShellRegistry {
    inner: Arc<RwLock<Vec<Arc<NativeShell>>>>,
}
impl ShellRegistry {
    pub(crate) fn register(&self, shell: Arc<NativeShell>) -> Result<()> {
        let mut shells = self
            .inner
            .write()
            .map_err(|_| Error::Denied("shell registry poisoned"))?;
        let node = shell.node_tool();
        if shells.iter().any(|current| current.node_tool() == node) {
            return Err(Error::Config("duplicate prepared shell interpreter".into()));
        }
        shells.push(shell);
        Ok(())
    }

    pub(crate) fn get(
        &self,
        node: aw_tools::filesystem::batch::NodeTool,
    ) -> Option<Arc<NativeShell>> {
        self.inner
            .read()
            .ok()?
            .iter()
            .find(|shell| shell.node_tool() == node)
            .cloned()
    }

    pub(crate) fn resolve(&self, name: &str) -> Option<aw_tools::filesystem::batch::NodeTool> {
        self.inner
            .read()
            .ok()?
            .iter()
            .find(|shell| shell.tool_name == name)
            .map(|shell| shell.node_tool())
    }

    pub(crate) fn node_tools(&self) -> Vec<aw_tools::filesystem::batch::NodeTool> {
        self.inner
            .read()
            .map(|shells| shells.iter().map(|shell| shell.node_tool()).collect())
            .unwrap_or_default()
    }
}

#[derive(Clone)]
pub(crate) struct NativeShell {
    root: PathBuf,
    executable: PathBuf,
    executable_identity: Arc<aw_platform::FileIdentity>,
    path: String,
    tool_name: &'static str,
    max_output_bytes: usize,
}
#[cfg(unix)]
pub(crate) type Bash = NativeShell;
#[cfg(windows)]
pub(crate) type PowerShell = NativeShell;
impl NativeShell {
    #[cfg(test)]
    pub fn new(root: PathBuf, executable: PathBuf) -> Result<Self> {
        Self::with_output_limit(root, executable, 48 * 1024)
    }

    pub fn with_output_limit(
        root: PathBuf,
        executable: PathBuf,
        max_output_bytes: usize,
    ) -> Result<Self> {
        if !(1..=48 * 1024).contains(&max_output_bytes) {
            return Err(Error::Config(
                "trusted shell output ceiling must be within 1..=49152 bytes".into(),
            ));
        }
        let executable = executable.canonicalize().map_err(|_| {
            Error::Config("trusted shell executable cannot be canonicalized".into())
        })?;
        #[cfg(unix)]
        let tool_name = "bash";
        #[cfg(windows)]
        let tool_name = match executable
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_ascii_lowercase())
            .as_deref()
        {
            Some("pwsh.exe" | "pwsh") => "pwsh",
            Some("powershell.exe" | "powershell") => "powershell",
            _ => {
                return Err(Error::Config(
                    "canonical trusted Windows shell executable must be pwsh or powershell".into(),
                ))
            }
        };
        if !executable.is_file() {
            return Err(Error::Config(
                "trusted shell executable is not a file".into(),
            ));
        }
        let executable_identity =
            Arc::new(aw_platform::FileIdentity::open(&executable).map_err(|_| {
                Error::Config("trusted shell executable identity unavailable".into())
            })?);
        Ok(Self {
            root,
            executable,
            executable_identity,
            path: std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
            tool_name,
            max_output_bytes,
        })
    }
}

impl NativeShell {
    pub(crate) fn node_tool(&self) -> aw_tools::filesystem::batch::NodeTool {
        use aw_tools::filesystem::batch::NodeTool;
        match self.tool_name {
            "bash" => NodeTool::Bash,
            "pwsh" => NodeTool::Pwsh,
            "powershell" => NodeTool::Powershell,
            _ => unreachable!("NativeShell constructor validates tool identity"),
        }
    }

    pub(crate) fn runtime_binding(&self) -> Result<[u8; 32]> {
        let (volume, object) = self
            .executable_identity
            .identity()
            .map_err(|_| Error::Denied("prepared shell executable identity unavailable"))?;
        let mut hash = Sha256::new();
        hash.update(b"aw-native-shell-runtime-v1\0");
        for value in [
            self.tool_name,
            self.executable.to_string_lossy().as_ref(),
            self.root.to_string_lossy().as_ref(),
        ] {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
        hash.update(volume.to_le_bytes());
        hash.update(object.to_le_bytes());
        hash.update((self.max_output_bytes as u64).to_le_bytes());
        Ok(hash.finalize().into())
    }

    pub(crate) fn revalidate(&self) -> Result<()> {
        if !self
            .executable_identity
            .is_current(&self.executable)
            .map_err(|_| Error::Denied("prepared shell executable cannot be revalidated"))?
        {
            return Err(Error::Denied("prepared shell executable changed"));
        }
        Ok(())
    }

    pub(crate) fn footprint(
        &self,
        prepared: &PreparedShell,
    ) -> aw_tools::filesystem::footprint::Footprint {
        use aw_tools::filesystem::{
            footprint::{Access, Extent, Footprint, Scope},
            write_hint::WriteHint,
        };
        let WriteHint::Known(paths) = &prepared.writes else {
            return Footprint::unknown(true, true);
        };
        #[cfg(windows)]
        if !paths.is_empty() {
            // Native filesystem footprints use invariant Windows namespace
            // folding. Shell write hints are advisory PathBuf values and do not
            // carry that backend key, so serialize rather than risk a case-alias
            // false negative against a native node.
            return Footprint::unknown(true, true);
        }
        let mut scopes = Vec::with_capacity(paths.len());
        for path in paths {
            let Ok(relative) = path.strip_prefix(&self.root) else {
                // Advisory scopes outside the configured workspace cannot prove
                // a narrow local conflict set; stay conservative rather than
                // treating the hint as authority.
                return Footprint::unknown(true, true);
            };
            let label = if relative.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                relative.to_string_lossy().replace('\\', "/")
            };
            scopes.push(Scope {
                path: label,
                access: Access::Write,
                extent: Extent::Subtree,
            });
        }
        Footprint::new(scopes, true, false).unwrap_or_else(|_| Footprint::unknown(true, true))
    }

    pub(crate) fn prepare(&self, v: &Value) -> Result<PreparedShell> {
        self.revalidate()?;
        self.validate(v)?;
        let args: Args = serde_json::from_value(v.clone())
            .map_err(|_| Error::Denied("invalid shell arguments"))?;
        let _ = args.parallel;
        let cwd = if args.dir == "." {
            self.root.clone()
        } else {
            let selected = self
                .root
                .join(&args.dir)
                .canonicalize()
                .map_err(|_| Error::Denied("shell dir does not exist"))?;
            if !selected.starts_with(&self.root) || !selected.is_dir() {
                return Err(Error::Denied("shell dir escapes granted root"));
            }
            selected
        };
        let writes = aw_tools::filesystem::write_hint::normalize(&cwd, args.writes.as_deref())
            .map_err(|_| Error::Denied("invalid shell writes hint"))?;
        #[cfg(unix)]
        let shell_args = vec![
            "--noprofile".into(),
            "--norc".into(),
            "-c".into(),
            args.command,
        ];
        #[cfg(windows)]
        let shell_args = vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            args.command,
        ];
        let environment = BTreeMap::from([
            ("PATH".into(), self.path.clone()),
            ("HOME".into(), self.root.to_string_lossy().into_owned()),
            ("LANG".into(), "C.UTF-8".into()),
        ]);
        #[cfg(windows)]
        let mut environment = environment;
        #[cfg(windows)]
        {
            environment.insert(
                "SystemRoot".into(),
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()),
            );
            environment.insert(
                "USERPROFILE".into(),
                self.root.to_string_lossy().into_owned(),
            );
        }
        Ok(PreparedShell {
            spec: PipeSpec {
                root: cwd,
                program: self.executable.clone(),
                args: shell_args,
                env: environment,
                stdin_open: args.stdin.is_some() || args.interactive,
            },
            stdin: args.stdin.map(String::into_bytes),
            interactive: args.interactive,
            timeout: Duration::from_secs(args.timeout_secs.unwrap_or(600)),
            max_output_bytes: args.max_output_bytes.unwrap_or(self.max_output_bytes),
            writes,
        })
    }
}
pub(crate) struct PreparedShell {
    pub spec: PipeSpec,
    pub stdin: Option<Vec<u8>>,
    pub interactive: bool,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub writes: aw_tools::filesystem::write_hint::WriteHint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    command: String,
    /// Model scheduling metadata, never an additional tool grant.
    #[serde(default)]
    parallel: bool,
    #[serde(default = "dot")]
    dir: String,
    stdin: Option<String>,
    #[serde(default)]
    interactive: bool,
    timeout_secs: Option<u64>,
    max_output_bytes: Option<usize>,
    writes: Option<Vec<String>>,
}
fn dot() -> String {
    ".".into()
}
impl Tool for NativeShell {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name:self.tool_name.into(),
            description:"Run explicitly granted trusted native shell with the OS user's rights (NOT a sandbox). Supports a workspace-relative dir, bounded stdin, interactive input through jobs_send, timeout_secs and per-stream max_output_bytes under host ceilings. Parallel waves require parallel=true and non-overlapping explicit writes scopes on every command. Writes are advisory scheduling hints, never permissions, sandboxing, or coordination guarantees.".into(),
            parameters:json!({"type":"object","properties":{
                "command":{"type":"string"},
                "dir":{"type":"string","description":"Working directory under the host-selected root, default ."},
                "stdin":{"type":"string","maxLength":16384},
                "interactive":{"type":"boolean","default":false},
                "timeout_secs":{"type":"integer","minimum":1,"maximum":600},
                "max_output_bytes":{"type":"integer","minimum":1,"maximum":self.max_output_bytes},
                "parallel":{"type":"boolean"},
                "writes":{"type":["array","null"],"maxItems":64,
                    "items":{"type":"string"}}
            },"required":["command"],"additionalProperties":false}),
            effect:ToolEffect::Process
        }
    }
    fn validate(&self, v: &Value) -> Result<()> {
        let args: Args = serde_json::from_value(v.clone())
            .map_err(|_| Error::Denied("bash accepts only a command string"))?;
        if args.command.trim().is_empty()
            || args.command.len() > 16_384
            || args.command.contains('\0')
        {
            return Err(Error::Denied("invalid or oversized bash command"));
        }
        if args.dir != "." {
            aw_platform::scoped_fs::validate_path(&args.dir)
                .map_err(|_| Error::Denied("shell dir must be relative to granted root"))?;
        }
        // The host does not treat a hint as authority or a guarantee that
        // arbitrary shell writes are limited to these paths.
        let effective = self.root.join(&args.dir);
        aw_tools::filesystem::write_hint::normalize(&effective, args.writes.as_deref())
            .map_err(|_| Error::Denied("invalid shell writes hint"))?;
        if args
            .stdin
            .as_deref()
            .is_some_and(|value| value.len() > 16_384)
        {
            return Err(Error::Limit("shell stdin exceeds 16 KiB"));
        }
        if args.timeout_secs.is_some_and(|v| !(1..=600).contains(&v))
            || args
                .max_output_bytes
                .is_some_and(|v| !(1..=self.max_output_bytes).contains(&v))
        {
            return Err(Error::Denied("shell limits must match the declared schema"));
        }
        Ok(())
    }
    fn can_run_parallel(&self, first: &Value, second: &Value) -> bool {
        use aw_tools::filesystem::write_hint::{normalize_names, overlaps};
        let scopes = |value: &Value| {
            let dir = value.get("dir").and_then(Value::as_str).unwrap_or(".");
            let cwd = self.root.join(dir);
            let mut items = Vec::new();
            let values = match value.get("writes") {
                None | Some(Value::Null) => {
                    return normalize_names(&cwd, Option::<std::iter::Empty<&str>>::None).ok()
                }
                Some(Value::Array(values)) => values,
                _ => return None,
            };
            if values.len() > aw_tools::filesystem::write_hint::MAX_SCOPES {
                return None;
            }
            for item in values {
                items.push(item.as_str()?);
            }
            normalize_names(&cwd, Some(items.into_iter())).ok()
        };
        match (scopes(first), scopes(second)) {
            (Some(a), Some(b)) => !overlaps(&a, &b),
            _ => false,
        }
    }
    fn execute<'a>(&'a self, ctx: ToolContext, v: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let prepared = self.prepare(v)?;
            let _ = ctx.result_budget(aw_agent_core::tool::MAX_TOOL_RESULT_BYTES)?;
            if ctx.cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let mut process = PipeProcess::spawn(prepared.spec).await?;
            if let Some(stdin) = &prepared.stdin {
                process.write(stdin).await?;
                process.close_stdin();
            }
            let receipt = process
                .wait(ctx.cancellation.cancelled(), prepared.timeout)
                .await;
            if !receipt.output_complete {
                return Err(Error::Transport(
                    "process output could not be fully drained; effects uncertain",
                ));
            }
            let (code, is_error) = match receipt.outcome {
                ProcessOutcome::Exited(status) => (status.to_string(), status != 0),
                ProcessOutcome::ExitedBySignal => ("signal".into(), true),
                ProcessOutcome::Cancelled => return Err(Error::Cancelled),
                ProcessOutcome::DeadlineExceeded => return Err(Error::Timeout),
                ProcessOutcome::Unknown => {
                    return Err(Error::Transport("process outcome uncertain"))
                }
            };
            let cap = prepared.max_output_bytes;
            let cut =
                |bytes: &[u8]| String::from_utf8_lossy(&bytes[..bytes.len().min(cap)]).into_owned();
            let content = format!(
                "exit: {}\nstdout:\n{}\nstderr:\n{}",
                code,
                cut(&receipt.output.stdout.head),
                cut(&receipt.output.stderr.head)
            );
            Ok(ToolOutput {
                content,
                is_error,
                truncated: receipt.output.stdout.truncated
                    || receipt.output.stderr.truncated
                    || receipt.output.stdout.head.len() > cap
                    || receipt.output.stderr.head.len() > cap,
            })
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn executable() -> PathBuf {
        std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_BASH").map(PathBuf::from))
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            })
            .expect("shell fixtures need bash; nix develop supplies AW_BASH")
    }
    fn ctx() -> ToolContext {
        let actor = PrincipalId::new("local").unwrap();
        ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "host-direct".into(),
            source: aw_agent_core::ToolSource::trusted_host(),
            effect: aw_agent_core::ToolEffect::Unknown,
            execution_site: "fixture".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: InvocationIdentity {
                actor: actor.clone(),
                account_id: "fixture".into(),
                account_owner: AccountOwner::User(actor.clone()),
                payer: BillingOwner::User(actor),
                credential_custody: CredentialCustody::UserDevice("fixture".into()),
                credential_version: None,
                inference_executor: "fixture".into(),
                model: "fixture".into(),
                grant_revision: 1,
            },
            scope: None,
            execution_owner: None,
            execution_scope: None,
        }
    }
    #[test]
    fn shell_registry_exposes_only_prepared_interpreters_and_conservative_footprints() {
        let shell = Arc::new(Bash::new(std::env::temp_dir(), executable()).unwrap());
        let registry = ShellRegistry::default();
        registry.register(shell.clone()).unwrap();
        assert_eq!(
            registry.resolve("bash"),
            Some(aw_tools::filesystem::batch::NodeTool::Bash)
        );
        assert_eq!(registry.resolve("pwsh"), None);
        let prepared = shell
            .prepare(&json!({"command":"true","writes":[]}))
            .unwrap();
        let footprint = shell.footprint(&prepared);
        assert!(footprint.unknown_read());
        assert!(!footprint.unknown_write());
        let unknown = shell.prepare(&json!({"command":"true"})).unwrap();
        assert!(shell.footprint(&unknown).unknown_write());
    }

    #[test]
    fn host_shell_stream_ceiling_narrows_schema_validation_and_default() {
        let tool = Bash::with_output_limit(std::env::temp_dir(), executable(), 1024).unwrap();
        assert_eq!(
            tool.spec().parameters["properties"]["max_output_bytes"]["maximum"],
            1024
        );
        assert!(tool
            .validate(&serde_json::json!({"command":"true","max_output_bytes":1025}))
            .is_err());
        assert_eq!(
            tool.prepare(&serde_json::json!({"command":"true"}))
                .unwrap()
                .max_output_bytes,
            1024
        );
    }

    #[test]
    fn prepared_shell_runtime_binding_is_stable_for_same_executable() {
        let a = Bash::new(std::env::temp_dir(), executable()).unwrap();
        let b = Bash::new(std::env::temp_dir(), executable()).unwrap();
        assert_eq!(a.runtime_binding().unwrap(), b.runtime_binding().unwrap());
        a.revalidate().unwrap();
    }

    #[test]
    fn trusted_shell_schema_and_validation_agree_on_caps() {
        let tool = Bash::new(std::env::temp_dir(), executable()).unwrap();
        assert_eq!(tool.spec().name, "bash");
        assert!(tool
            .validate(&json!({"command":"true","timeout_secs":600,
            "max_output_bytes":49_152}))
            .is_ok());
        assert!(tool
            .validate(&json!({"command":"true","timeout_secs":601}))
            .is_err());
        assert!(tool
            .validate(&json!({"command":"true","max_output_bytes":49_153}))
            .is_err());
        assert!(tool
            .validate(&json!({"command":"true","timeout_secs":0}))
            .is_err());
    }

    #[tokio::test]
    async fn shell_is_real_and_captures_output() {
        let b = Bash::new(std::env::temp_dir(), executable()).unwrap();
        let result = b
            .execute(
                ctx(),
                &json!({"command":"printf 'fixture-out'; printf 'fixture-err' >&2"}),
            )
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("fixture-out") && result.content.contains("fixture-err"));
    }
    #[tokio::test]
    async fn nonzero_exit_is_tool_error() {
        let b = Bash::new(std::env::temp_dir(), executable()).unwrap();
        assert!(
            b.execute(ctx(), &json!({"command":"exit 9"}))
                .await
                .unwrap()
                .is_error
        );
    }
    #[tokio::test]
    async fn cancellation_returns_without_claiming_rollback() {
        let b = Bash::new(std::env::temp_dir(), executable()).unwrap();
        let c = ctx();
        let token = c.cancellation.clone();
        let cancel = async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            token.cancel();
        };
        let args = json!({"command":"while :; do :; done"});
        let (result, ()) = tokio::join!(b.execute(c, &args), cancel);
        assert!(matches!(result, Err(Error::Cancelled)));
    }
    #[test]
    fn explicit_model_parallel_preference_is_boolean_and_does_not_change_effect() {
        let b = Bash::new(std::env::temp_dir(), executable()).unwrap();
        assert!(b
            .validate(&json!({"command":"printf ok","parallel":true}))
            .is_ok());
        assert!(b
            .validate(&json!({"command":"printf ok","parallel":false}))
            .is_ok());
        assert!(b.validate(&json!({"command":"printf ok"})).is_ok());
        assert!(b
            .validate(&json!({"command":"printf ok","parallel":"true"}))
            .is_err());
        assert!(b
            .validate(&json!({"command":"printf ok","parallel":true,"cwd":"/tmp"}))
            .is_err());
        assert_eq!(b.spec().effect, ToolEffect::Process);
    }
    #[test]
    fn shell_parallel_waves_require_disjoint_explicit_write_scopes() {
        let shell = Bash::new(std::env::temp_dir(), executable()).unwrap();
        let command = |writes: Value, dir: &str| {
            json!({
                "command":"true","parallel":true,
                "dir":dir,"writes":writes,
            })
        };
        let unknown = json!({"command":"true","parallel":true});
        let a = command(json!(["src/a.rs"]), ".");
        let b = command(json!(["src/b.rs"]), ".");
        let overlap = command(json!(["src"]), ".");
        let nested = command(json!(["a.rs"]), "src");
        let empty = command(json!([]), ".");
        assert!(shell.can_run_parallel(&a, &b));
        assert!(!shell.can_run_parallel(&a, &overlap));
        assert!(!shell.can_run_parallel(&a, &nested));
        assert!(!shell.can_run_parallel(&a, &unknown));
        assert!(!shell.can_run_parallel(&unknown, &b));
        assert!(shell.can_run_parallel(&a, &empty));
    }
    #[test]
    fn advisory_shell_write_footprints_are_checked_but_not_grants() {
        let shell = Bash::new(std::env::temp_dir(), executable()).unwrap();
        assert!(shell
            .validate(&json!({"command":"true","writes":null}))
            .is_ok());
        assert!(shell
            .validate(&json!({"command":"true","writes":[]}))
            .is_ok());
        assert!(shell
            .validate(&json!({"command":"true","writes":["src",
            "src/foo.rs"]}))
            .is_ok());
        assert!(shell
            .validate(&json!({"command":"true","writes":["*.rs"]}))
            .is_err());
        assert!(shell
            .validate(&json!({"command":"true","writes":["src/$X"]}))
            .is_err());
        assert!(shell.spec().parameters["properties"]
            .get("writes")
            .is_some());
        assert_eq!(shell.spec().effect, ToolEffect::Process);
    }
    #[test]
    fn arguments_are_not_arbitrary_program_or_environment() {
        let b = Bash::new(std::env::temp_dir(), executable()).unwrap();
        for v in [
            json!({"command":"ok","env":{"TOKEN":"secret"}}),
            json!({"command":"","cwd":"/"}),
            json!({"command":""}),
        ] {
            assert!(b.validate(&v).is_err());
        }
    }
}
