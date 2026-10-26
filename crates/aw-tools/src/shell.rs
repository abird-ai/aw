//! The foreground trusted Bash tool is a granted Process effect. Its actual
//! subprocess uses the SAME host process lifecycle/capture service as other
//! pipe-based process actors. No alternate Bash-specific reaper/capture loop.
//! Same-UID trusted shell access is emphatically NOT an OS sandbox.
use crate::process::{Outcome as ProcessOutcome, PipeProcess, PipeSpec};
use aw_agent_core::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) struct NativeShell {
    root: PathBuf,
    executable: PathBuf,
    path: String,
}
#[cfg(unix)]
pub(crate) type Bash = NativeShell;
#[cfg(windows)]
pub(crate) type PowerShell = NativeShell;
impl NativeShell {
    pub fn new(root: PathBuf, executable: PathBuf) -> Result<Self> {
        Ok(Self {
            root,
            executable,
            path: std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        })
    }
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
    timeout_secs: Option<u64>,
    max_output_bytes: Option<usize>,
}
fn dot() -> String {
    ".".into()
}
impl Tool for NativeShell {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name:"bash".into(),
            description:"Run explicitly granted trusted native shell with the OS user's rights (NOT a sandbox). Supports a workspace-relative dir, stdin, timeout_secs and per-stream max_output_bytes under host ceilings. Only parallel=true opts adjacent independent commands into AW's model-directed Process waves; writes may race.".into(),
            parameters:json!({"type":"object","properties":{
                "command":{"type":"string"},
                "dir":{"type":"string","description":"Working directory under the host-selected root, default ."},
                "stdin":{"type":"string","maxLength":16384},
                "timeout_secs":{"type":"integer","minimum":1,"maximum":600},
                "max_output_bytes":{"type":"integer","minimum":1,"maximum":49152},
                "parallel":{"type":"boolean"}
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
        if args
            .stdin
            .as_deref()
            .is_some_and(|value| value.len() > 16_384)
        {
            return Err(Error::Limit("shell stdin exceeds 16 KiB"));
        }
        if args.timeout_secs == Some(0) || args.max_output_bytes == Some(0) {
            return Err(Error::Denied("shell timeout/output limit must be positive"));
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, v: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(v)?;
            let args: Args = serde_json::from_value(v.clone())
                .map_err(|_| Error::Denied("invalid bash arguments"))?;
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
            if ctx.cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            // Program, cwd and environment come from the host-created tool,
            // not from the model-supplied JSON. The Agent has journaled Tool
            // intent and rechecked exact Process authority before execute.
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
            let spec = PipeSpec {
                root: cwd,
                program: self.executable.clone(),
                args: shell_args,
                env: environment,
                stdin_open: args.stdin.is_some(),
            };
            let mut process = PipeProcess::spawn(spec).await?;
            if let Some(stdin) = &args.stdin {
                process.write(stdin.as_bytes()).await?;
                process.close_stdin();
            }
            let receipt = process
                .wait(
                    ctx.cancellation.cancelled(),
                    std::time::Duration::from_secs(args.timeout_secs.unwrap_or(600).clamp(1, 600)),
                )
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
            let cap = args
                .max_output_bytes
                .unwrap_or(48 * 1024)
                .clamp(1, 48 * 1024);
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
            execution_site: "fixture".into(),
            cancellation: CancellationToken::new(),
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
        }
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
        let b = Bash::new(std::env::temp_dir(), PathBuf::from("/not/executed")).unwrap();
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
    fn arguments_are_not_arbitrary_program_or_environment() {
        let b = Bash::new(std::env::temp_dir(), PathBuf::from("/not/executed")).unwrap();
        for v in [
            json!({"command":"ok","env":{"TOKEN":"secret"}}),
            json!({"command":"","cwd":"/"}),
            json!({"command":""}),
        ] {
            assert!(b.validate(&v).is_err());
        }
    }
}
