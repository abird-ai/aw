//! The foreground trusted Bash tool is a granted Process effect. Its actual
//! subprocess uses the SAME host process lifecycle/capture service as other
//! pipe-based process actors. No alternate Bash-specific reaper/capture loop.
//! Same-UID trusted shell access is emphatically NOT an OS sandbox.
use crate::process::{Outcome as ProcessOutcome, PipeProcess, PipeSpec};
use aw_agent_core::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) struct Bash {
    root: PathBuf,
    executable: PathBuf,
    path: String,
}
impl Bash {
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
}
impl Tool for Bash {
    fn spec(&self) -> ToolSpec {
        ToolSpec{name:"bash".into(),description:"Run an explicitly granted TRUSTED Bash command with the OS user's rights (not a sandbox). Optionally set parallel=true ONLY for independent commands: adjacent opted-in commands may overlap and race on shared files unless host safety constraints force serial execution. Omit/false for ordered execution. The host keeps intent/receipt ordering and never retries ambiguous effects.".into(),parameters:json!({"type":"object","properties":{"command":{"type":"string"},"parallel":{"type":"boolean","description":"Model may overlap independent adjacent Bash calls; host policy may constrain concurrency. Set false/omit for dependent commands."}},"required":["command"],"additionalProperties":false}),effect:ToolEffect::Process}
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
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, v: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(v)?;
            let args: Args = serde_json::from_value(v.clone())
                .map_err(|_| Error::Denied("invalid bash arguments"))?;
            let _ = args.parallel;
            if ctx.cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            // Program, cwd and environment come from the host-created tool,
            // not from the model-supplied JSON. The Agent has journaled Tool
            // intent and rechecked exact Process authority before execute.
            let spec = PipeSpec {
                root: self.root.clone(),
                program: self.executable.clone(),
                args: vec![
                    "--noprofile".into(),
                    "--norc".into(),
                    "-c".into(),
                    args.command,
                ],
                env: BTreeMap::from([
                    ("PATH".into(), self.path.clone()),
                    ("HOME".into(), self.root.to_string_lossy().into_owned()),
                    ("LANG".into(), "C.UTF-8".into()),
                ]),
                stdin_open: false,
            };
            let process = PipeProcess::spawn(spec).await?;
            let receipt = process
                .wait(
                    ctx.cancellation.cancelled(),
                    std::time::Duration::from_secs(600),
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
            let content = format!(
                "exit: {}\nstdout:\n{}\nstderr:\n{}",
                code,
                String::from_utf8_lossy(&receipt.output.stdout.head),
                String::from_utf8_lossy(&receipt.output.stderr.head)
            );
            Ok(ToolOutput {
                content,
                is_error,
                truncated: receipt.output.stdout.truncated || receipt.output.stderr.truncated,
            })
        })
    }
}

#[cfg(test)]
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
