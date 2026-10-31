//! Host-only Linux Bubblewrap launch preparation over pinned filesystem
//! capabilities. No model ToolSpec, implicit shell grant, or fallback to
//! unsandboxed execution. The host must journal Process intent first.

use crate::sandbox::ValidatedSandboxPolicy;
use std::{
    collections::BTreeSet,
    ffi::{CString, OsString},
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
        unix::fs::PermissionsExt,
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command},
};
#[derive(Debug)]
pub enum BwrapError {
    InvalidPath,
    InvalidLauncher,
    InvalidRuntime,
    ConflictingMounts,
    ProgramNotMounted,
    TooManyArguments,
    SandboxExited(Option<i32>),
    Io(std::io::Error),
}
impl From<std::io::Error> for BwrapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
pub struct RuntimeDirectory<'a> {
    pub source: &'a Path,
    pub destination: &'a Path,
}
/// RAII child owner; a host may wait, inspect or terminate it, but dropping
/// a still-running sandbox does not silently detach a process.
pub struct BwrapChild {
    child: Option<Child>,
    finished: bool,
}
impl BwrapChild {
    pub fn id(&self) -> u32 {
        self.child.as_ref().expect("child owned").id()
    }
    pub fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let status = self.child.as_mut().expect("child owned").wait()?;
        self.finished = true;
        Ok(status)
    }
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let status = self.child.as_mut().expect("child owned").try_wait()?;
        if status.is_some() {
            self.finished = true;
        }
        Ok(status)
    }
    /// Unlike spawn(), this observes the actual launcher/command outcome.
    /// A nonzero exit is not proof that an isolated payload ran; the host
    /// records its factual status and must never retry effects implicitly.
    pub fn wait_success(&mut self) -> Result<(), BwrapError> {
        let status = self.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(BwrapError::SandboxExited(status.code()))
        }
    }
    pub fn terminate(&mut self) -> std::io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.child.as_mut().expect("child owned").kill()?;
        self.wait().map(|_| ())
    }
}
impl Drop for BwrapChild {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = child.kill();
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        // SIGKILL is asynchronous. One try_wait can return None and leave a
        // zombie; a detached reaper owns the Child until waitpid completes.
        // No model/background process authority is created by this thread.
        let pending = std::sync::Arc::new(std::sync::Mutex::new(Some(child)));
        let background = pending.clone();
        if std::thread::Builder::new()
            .name("aw-bwrap-reap".into())
            .spawn(move || {
                let mut child = background.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(ref mut child) = child {
                    let _ = child.wait();
                }
            })
            .is_err()
        {
            // Extremely rare thread creation failure: prefer bounded
            // task cleanup correctness to silently orphaning a zombie.
            if let Some(mut child) = pending.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = child.wait();
            }
        }
    }
}
pub struct BwrapPlan {
    // Opened by openat2(NO_SYMLINKS); kernel file identity is retained even
    // when another actor renames the original pathname.
    held: Vec<File>,
    args: Vec<OsString>,
}
impl BwrapPlan {
    pub fn prepare(
        launcher: &Path,
        workspace: &ValidatedSandboxPolicy,
        runtime: &[RuntimeDirectory<'_>],
        executable: &Path,
        arguments: &[OsString],
    ) -> Result<Self, BwrapError> {
        if runtime.len() > 16
            || arguments.len() > 32
            || arguments
                .iter()
                .any(|s| s.as_bytes().len() > 16_384 || s.as_bytes().contains(&0))
        {
            return Err(BwrapError::TooManyArguments);
        }
        if !absolute_components(executable) || executable == Path::new("/") {
            return Err(BwrapError::InvalidPath);
        }
        let pinned_launcher = pin(launcher, false).map_err(|_| BwrapError::InvalidLauncher)?;
        let mut held = vec![pinned_launcher];
        let workspace_fd = workspace.pinned_root().try_clone()?;
        let root_fd = workspace_fd.as_raw_fd();
        held.push(workspace_fd);
        let mut destinations = Vec::new();
        let mut sources = Vec::new();
        for mapping in runtime {
            let dest = mapping.destination;
            // Never allow a runtime convenience mount to expose the entire
            // host root; it is not a system-runtime directory.
            if mapping.source == Path::new("/") {
                return Err(BwrapError::InvalidRuntime);
            }
            if !absolute_components(dest)
                || dest == Path::new("/")
                || dest.starts_with("/workspace")
                || dest.starts_with("/proc")
                || dest.starts_with("/dev")
                || dest.starts_with("/tmp")
            {
                return Err(BwrapError::InvalidRuntime);
            }
            if destinations
                .iter()
                .any(|old: &PathBuf| old.starts_with(dest) || dest.starts_with(old))
            {
                return Err(BwrapError::ConflictingMounts);
            }
            let file = pin(mapping.source, true).map_err(|_| BwrapError::InvalidRuntime)?;
            destinations.push(dest.to_owned());
            sources.push(file);
        }
        let Some((runtime_index, runtime_dest)) = destinations
            .iter()
            .enumerate()
            .find(|(_, dest)| executable.starts_with(dest) && executable != dest.as_path())
        else {
            return Err(BwrapError::ProgramNotMounted);
        };
        let relative = executable
            .strip_prefix(runtime_dest)
            .map_err(|_| BwrapError::ProgramNotMounted)?;
        // Pin the host-selected executable itself. Runtime mount pathnames
        // may be renamed after preparation; program identity is not inferred
        // from a writable directory or an untrusted symlink.
        let command_file = pin_runtime_executable(&sources[runtime_index], relative)?;

        let mut args = vec![
            "--die-with-parent".into(),
            "--new-session".into(),
            "--unshare-all".into(),
            "--clearenv".into(),
            "--cap-drop".into(),
            "ALL".into(),
            // Nested user namespaces could reopen mount/permission behavior
            // after the host constructed an explicit sandbox scope.
            "--disable-userns".into(),
            "--hostname".into(),
            "aw-sandbox".into(),
            "--proc".into(),
            "/proc".into(),
            "--dev".into(),
            "/dev".into(),
            "--tmpfs".into(),
            "/tmp".into(),
            "--dir".into(),
            "/workspace".into(),
        ];
        if workspace.network() {
            args.push("--share-net".into());
        }
        for (key, value) in [
            ("HOME", "/workspace"),
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "C.UTF-8"),
        ] {
            args.extend(["--setenv".into(), key.into(), value.into()]);
        }
        args.extend([
            "--ro-bind-fd".into(),
            root_fd.to_string().into(),
            "/workspace".into(),
        ]);
        // Create mount parents outside /workspace before binding runtimes.
        let mut parents = BTreeSet::<PathBuf>::new();
        for target in &destinations {
            let mut parent = target.as_path();
            while let Some(next) = parent.parent() {
                if next == Path::new("/") {
                    break;
                }
                parents.insert(next.to_owned());
                parent = next;
            }
            parents.insert(target.clone());
        }
        for path in parents {
            args.push("--dir".into());
            args.push(path.into_os_string());
        }
        for (dir, file) in destinations.iter().zip(&sources) {
            args.push("--ro-bind-fd".into());
            args.push(file.as_raw_fd().to_string().into());
            args.push(dir.as_os_str().to_owned());
        }
        held.extend(sources);
        args.extend([
            "--ro-bind-fd".into(),
            command_file.as_raw_fd().to_string().into(),
            executable.as_os_str().to_owned(),
        ]);
        held.push(command_file);
        for (root, pinned) in workspace
            .writable_roots()
            .iter()
            .zip(workspace.pinned_writes())
        {
            let source = pinned.try_clone()?;
            let relative = root
                .strip_prefix(workspace.root())
                .map_err(|_| BwrapError::InvalidPath)?;
            let target = Path::new("/workspace").join(relative);
            args.extend([
                "--bind-fd".into(),
                source.as_raw_fd().to_string().into(),
                target.into_os_string(),
            ]);
            held.push(source);
        }
        args.extend(["--chdir".into(), "/workspace".into(), "--".into()]);
        args.push(executable.as_os_str().to_owned());
        args.extend(arguments.iter().cloned());
        Ok(Self { held, args })
    }
    pub fn arguments(&self) -> &[OsString] {
        &self.args
    }
    /// Spawn only after the original AW host journals Process ToolStarted and
    /// verifies the exact source/site/effect grant. The library does not
    /// implement a standalone trust policy or automatic job durability.
    pub fn spawn(&self) -> Result<BwrapChild, BwrapError> {
        use std::os::unix::process::CommandExt;
        let pinned_exe = format!("/proc/self/fd/{}", self.held[0].as_raw_fd());
        let mut command = Command::new(pinned_exe);
        command.args(&self.args).env_clear();
        let fds = self.held.iter().map(AsRawFd::as_raw_fd).collect::<Vec<_>>();
        // SAFETY: fcntl is async-signal-safe; pre_exec is the only child path
        // that changes CLOEXEC on the already-open, pinned bwrap mount FDs.
        unsafe {
            command.pre_exec(move || {
                for fd in &fds {
                    let flags = libc::fcntl(*fd, libc::F_GETFD);
                    if flags < 0 || libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn().map_err(BwrapError::Io)?;
        Ok(BwrapChild {
            child: Some(child),
            finished: false,
        })
    }
}

fn absolute_components(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}
pub(crate) fn pin(path: &Path, directory: bool) -> Result<File, BwrapError> {
    if !absolute_components(path) {
        return Err(BwrapError::InvalidPath);
    }
    let name = CString::new(path.as_os_str().as_bytes()).map_err(|_| BwrapError::InvalidPath)?;
    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }
    // openat2 forbids symlinks anywhere in the path, not only the leaf.
    const RESOLVE_NO_SYMLINKS: u64 = 0x04;
    let flags =
        (libc::O_CLOEXEC | libc::O_PATH | if directory { libc::O_DIRECTORY } else { 0 }) as u64;
    let how = OpenHow {
        flags,
        mode: 0,
        resolve: RESOLVE_NO_SYMLINKS,
    };
    // SAFETY: valid NUL path and fixed-layout kernel input; returned FD is
    // checked and becomes an owned File exactly once.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::AT_FDCWD,
            name.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        ) as i32
    };
    if fd < 0 {
        return Err(BwrapError::Io(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !(if directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    }) {
        return Err(BwrapError::InvalidPath);
    }
    Ok(file)
}
/// Descriptor-relative no-follow/openat2 below the approved runtime root.
fn pin_runtime_executable(runtime: &File, relative: &Path) -> Result<File, BwrapError> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::RootDir
            )
        })
    {
        return Err(BwrapError::InvalidRuntime);
    }
    let name =
        CString::new(relative.as_os_str().as_bytes()).map_err(|_| BwrapError::InvalidRuntime)?;
    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }
    const RESOLVE_BENEATH: u64 = 0x08;
    const RESOLVE_NO_SYMLINKS: u64 = 0x04;
    let how = OpenHow {
        flags: (libc::O_CLOEXEC | libc::O_PATH) as u64,
        mode: 0,
        resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS,
    };
    // SAFETY: pinned directory, NUL-terminated relative file name, and
    // fixed-layout kernel input. No fallback to unsafe path resolution.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            runtime.as_raw_fd(),
            name.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        ) as i32
    };
    if fd < 0 {
        return Err(BwrapError::InvalidRuntime);
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(BwrapError::InvalidRuntime);
    }
    Ok(file)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxPolicy;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    static SEQ: AtomicU64 = AtomicU64::new(1);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "abird-bwrap-plan-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn dir(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            fs::create_dir_all(&p).unwrap();
            p
        }
        fn launcher(&self) -> PathBuf {
            let p = self.0.join("bwrap");
            fs::write(&p, b"binary").unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
            p
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn checked_wait_does_not_invent_success_on_launcher_failure() {
        let child = Command::new("sh").args(["-c", "exit 42"]).spawn().unwrap();
        let mut owned = BwrapChild {
            child: Some(child),
            finished: false,
        };
        assert!(matches!(
            owned.wait_success(),
            Err(BwrapError::SandboxExited(Some(42)))
        ));
        assert!(owned.finished);
    }
    #[test]
    fn dropping_child_reaps_after_asynchronous_kill() {
        use std::{process::Command, time::Duration};
        let child = Command::new("sh")
            .args(["-c", "exec sleep 30"])
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        drop(BwrapChild {
            child: Some(child),
            finished: false,
        });
        let mut reaped = false;
        for _ in 0..200 {
            // SAFETY: probing a PID does not consume its exit status or race
            // the detached reaper's waitpid.
            let rc = unsafe { libc::kill(pid, 0) };
            if rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                reaped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(reaped, "child must disappear after the reaper's wait");
    }
    #[test]
    fn bwrap_plan_has_no_ambient_root_mount_or_network_grant() {
        let temp = Temp::new();
        let root = temp.dir("workspace");
        let write = temp.dir("workspace/src");
        let runtime = temp.dir("runtime");
        fs::create_dir_all(runtime.join("bin")).unwrap();
        let executable_file = runtime.join("bin/echo");
        fs::write(&executable_file, b"trusted-host-executable").unwrap();
        fs::set_permissions(&executable_file, fs::Permissions::from_mode(0o700)).unwrap();
        let executable = Path::new("/usr/bin/echo");
        let checked = SandboxPolicy {
            root: &root,
            network: false,
            writable_roots: &[write.as_path()],
            allow_root_write: false,
        }
        .validate()
        .unwrap();
        let args = [OsString::from("--model-controlled-argument")];
        let plan = BwrapPlan::prepare(
            &temp.launcher(),
            &checked,
            &[RuntimeDirectory {
                source: &runtime,
                destination: Path::new("/usr"),
            }],
            executable,
            &args,
        )
        .unwrap();
        let argv = plan
            .arguments()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(argv.contains(&"--unshare-all".into()));
        assert!(argv.contains(&"--disable-userns".into()));
        assert!(!argv.contains(&"--share-net".into()));
        assert!(argv.contains(&"--ro-bind-fd".into()));
        assert!(argv.contains(&"--bind-fd".into()));
        assert!(!argv
            .windows(2)
            .any(|pair| pair[0] == "--ro-bind" && pair[1] == "/"));
        let separator = argv.iter().position(|s| s == "--").unwrap();
        assert_eq!(argv[separator + 1], "/usr/bin/echo");
        assert_eq!(argv[separator + 2], "--model-controlled-argument");
    }
    #[test]
    fn validated_write_directory_keeps_original_inode_after_path_replacement() {
        use std::os::unix::fs::MetadataExt;
        let temp = Temp::new();
        let root = temp.dir("workspace");
        let write = temp.dir("workspace/src");
        let original = fs::metadata(&write).unwrap().ino();
        let checked = SandboxPolicy {
            root: &root,
            network: false,
            writable_roots: &[write.as_path()],
            allow_root_write: false,
        }
        .validate()
        .unwrap();
        fs::rename(&write, root.join("old_src")).unwrap();
        fs::create_dir(&write).unwrap();
        assert_ne!(original, fs::metadata(&write).unwrap().ino());
        let runtime = temp.dir("runtime");
        fs::create_dir_all(runtime.join("bin")).unwrap();
        let executable_file = runtime.join("bin/hello");
        fs::write(&executable_file, b"trusted").unwrap();
        fs::set_permissions(&executable_file, fs::Permissions::from_mode(0o700)).unwrap();
        let plan = BwrapPlan::prepare(
            &temp.launcher(),
            &checked,
            &[RuntimeDirectory {
                source: &runtime,
                destination: Path::new("/runtime"),
            }],
            Path::new("/runtime/bin/hello"),
            &[],
        )
        .unwrap();
        let args = plan
            .arguments()
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();
        let fd = args
            .windows(3)
            .find(|parts| parts[0] == "--bind-fd" && parts[2] == "/workspace/src")
            .unwrap()[1]
            .parse::<i32>()
            .unwrap();
        let mounted = fs::metadata(format!("/proc/self/fd/{fd}")).unwrap().ino();
        assert_eq!(mounted, original, "cannot substitute a new write directory");
    }
    #[test]
    fn runtime_executable_must_exist_and_not_be_a_symlink() {
        use std::os::unix::fs::symlink;
        let temp = Temp::new();
        let root = temp.dir("workspace");
        let runtime = temp.dir("runtime");
        fs::create_dir_all(runtime.join("bin")).unwrap();
        fs::write(runtime.join("bin/original"), b"original").unwrap();
        fs::set_permissions(
            runtime.join("bin/original"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        symlink("original", runtime.join("bin/alias")).unwrap();
        let checked = SandboxPolicy {
            root: &root,
            network: false,
            writable_roots: &[],
            allow_root_write: false,
        }
        .validate()
        .unwrap();
        for executable in ["/runtime/bin/missing", "/runtime/bin/alias"] {
            let result = BwrapPlan::prepare(
                &temp.launcher(),
                &checked,
                &[RuntimeDirectory {
                    source: &runtime,
                    destination: Path::new("/runtime"),
                }],
                Path::new(executable),
                &[],
            );
            assert!(matches!(result, Err(BwrapError::InvalidRuntime)));
        }
    }
    #[test]
    fn host_root_cannot_be_aliased_as_a_runtime_mount() {
        let temp = Temp::new();
        let root = temp.dir("workspace");
        let checked = SandboxPolicy {
            root: &root,
            network: false,
            writable_roots: &[],
            allow_root_write: false,
        }
        .validate()
        .unwrap();
        let launcher = temp.launcher();
        let bad = BwrapPlan::prepare(
            &launcher,
            &checked,
            &[RuntimeDirectory {
                source: Path::new("/"),
                destination: Path::new("/runtime"),
            }],
            Path::new("/runtime/bin/sh"),
            &[],
        );
        assert!(matches!(bad, Err(BwrapError::InvalidRuntime)));
    }
    #[test]
    fn refuses_unmounted_program_and_overlapping_runtime_mounts() {
        let temp = Temp::new();
        let root = temp.dir("workspace");
        let runtime = temp.dir("runtime");
        let checked = SandboxPolicy {
            root: &root,
            network: false,
            writable_roots: &[],
            allow_root_write: false,
        }
        .validate()
        .unwrap();
        let launcher = temp.launcher();
        let denied = BwrapPlan::prepare(
            &launcher,
            &checked,
            &[RuntimeDirectory {
                source: &runtime,
                destination: Path::new("/usr"),
            }],
            Path::new("/etc/passwd"),
            &[],
        );
        assert!(matches!(denied, Err(BwrapError::ProgramNotMounted)));
        let runtime2 = temp.dir("runtime2");
        let overlapping = BwrapPlan::prepare(
            &launcher,
            &checked,
            &[
                RuntimeDirectory {
                    source: &runtime,
                    destination: Path::new("/usr"),
                },
                RuntimeDirectory {
                    source: &runtime2,
                    destination: Path::new("/usr/bin"),
                },
            ],
            Path::new("/usr/bin/echo"),
            &[],
        );
        assert!(matches!(overlapping, Err(BwrapError::ConflictingMounts)));
    }
}
