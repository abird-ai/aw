use crate::{Error, Result, MAX_WIRE_BYTES};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

/// Capture bounded output and time. No shell, pager, hooks, ambient Git options,
/// credential helpers, terminal prompt, fsmonitor or model/provider environment.
pub(crate) fn run(
    git: &Path,
    root: &Path,
    args: &[OsString],
    input: Option<&[u8]>,
    timeout: Duration,
    allow_one: bool,
) -> Result<Vec<u8>> {
    let mut command = Command::new(git);
    let git_parent = git
        .parent()
        .ok_or(Error::Invalid("Git executable has no parent"))?;
    let safe_path = std::env::join_paths([git_parent])
        .map_err(|_| Error::Invalid("Git executable path cannot form PATH"))?;
    command
        .current_dir(root)
        .env_clear()
        .env("PATH", safe_path)
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", aw_platform::null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args([
            OsString::from("--no-pager"),
            OsString::from("--literal-pathspecs"),
            OsString::from("-c"),
            OsString::from("core.fsmonitor=false"),
            OsString::from("-c"),
            OsString::from(format!("core.hooksPath={}", aw_platform::null_device())),
            OsString::from("-c"),
            OsString::from("core.untrackedCache=false"),
            OsString::from("-c"),
            OsString::from("core.pager=cat"),
            OsString::from("-c"),
            OsString::from("credential.helper="),
        ])
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    aw_platform::configure_child_group(&mut command);
    let mut child = ChildGuard(command.spawn()?);
    let overflow = Arc::new(AtomicBool::new(false));
    let capture = |pipe: Box<dyn Read + Send>, overflow: Arc<AtomicBool>| {
        thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut out = Vec::new();
            let mut pipe = pipe;
            let mut buf = [0_u8; 8192];
            loop {
                let n = pipe.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                if out.len().saturating_add(n) > MAX_WIRE_BYTES {
                    overflow.store(true, Ordering::Release);
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            Ok(out)
        })
    };
    let stdout = capture(
        Box::new(child.stdout.take().ok_or(Error::Invalid("stdout pipe"))?),
        overflow.clone(),
    );
    let stderr = capture(
        Box::new(child.stderr.take().ok_or(Error::Invalid("stderr pipe"))?),
        overflow.clone(),
    );
    let writer = input.map(|data| {
        let data = data.to_vec();
        let pipe = child.stdin.take();
        thread::spawn(move || -> std::io::Result<()> {
            if let Some(mut pipe) = pipe {
                pipe.write_all(&data)?;
            }
            Ok(())
        })
    });
    let start = Instant::now();
    let mut stopped = None;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if overflow.load(Ordering::Acquire) || start.elapsed() >= timeout {
            stopped = Some(if overflow.load(Ordering::Acquire) {
                Error::Limit("Git wire output")
            } else {
                Error::Timeout
            });
            // Terminate the platform-owned child tree before waiting so no
            // descendant retains capture pipes indefinitely.
            let _ = aw_platform::terminate_child_tree(&mut child);
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let out = stdout
        .join()
        .map_err(|_| Error::Invalid("Git capture panicked"))??;
    let _err = stderr
        .join()
        .map_err(|_| Error::Invalid("Git error capture panicked"))??;
    let input_result = writer
        .map(|w| {
            w.join()
                .map_err(|_| Error::Invalid("Git input worker panicked"))
        })
        .transpose()?;
    if let Some(error) = stopped {
        return Err(error);
    }
    if overflow.load(Ordering::Acquire) {
        return Err(Error::Limit("Git wire output"));
    }
    let code = status.code().unwrap_or(-1);
    if !(status.success() || allow_one && code == 1) {
        return Err(Error::Exit(code));
    }
    if let Some(result) = input_result {
        result?;
    }
    Ok(out)
}

// Keep early pipe/wait/I/O failures from dropping an unreaped Git process.
// This is our direct child and process group, not arbitrary process-tree isolation.
struct ChildGuard(std::process::Child);
impl std::ops::Deref for ChildGuard {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = aw_platform::terminate_child_tree(&mut self.0);
            let _ = self.0.wait();
        }
    }
}
