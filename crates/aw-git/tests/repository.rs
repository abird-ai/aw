//! Real Git subprocess fixtures. Run on the Rust host; no network/model required.
use aw_git::*;
use std::os::unix::ffi::OsStringExt;
use std::{
    ffi::OsString,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture {
    parent: PathBuf,
    root: PathBuf,
    git: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let parent = std::env::temp_dir().join(format!(
            "aw-git-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&parent).unwrap();
        let root = parent.join("repo");
        std::fs::create_dir(&root).unwrap();
        let git = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let f = Self { parent, root, git };
        f.run(&["init", "--initial-branch=main"]);
        f.run(&["config", "user.name", "Fixture"]);
        f.run(&["config", "user.email", "fixture@example.invalid"]);
        f.write("a.txt", b"one\ntwo\n");
        f.run(&["add", "a.txt"]);
        f.run(&["commit", "-m", "fixture", "--no-gpg-sign"]);
        f
    }
    fn cmd(&self) -> Command {
        let mut c = Command::new(&self.git);
        c.current_dir(&self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        let o = self.cmd().args(args).output().unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        o
    }
    fn repo(&self) -> Repository {
        Repository::discover(&self.git, &self.root).unwrap()
    }
    #[cfg(feature = "source-driver")]
    fn interrupt_private_index(&self, verb: &str) -> aw_git::source_driver::GitSourceDriver {
        use std::os::unix::fs::PermissionsExt;
        // A trusted fixture executable reports failure after real Git writes
        // the original private index (or completes its checkout).
        let shell = std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| option_env!("AW_DEFAULT_BASH").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/bin/sh"));
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let wrapper = self.parent.join("interrupt-git");
        let git = quote(self.git.to_str().unwrap());
        std::fs::write(&wrapper, format!(
            "#!{}\ncase \"${{GIT_INDEX_FILE:-}}\" in\n  */aw-capture-*/index-1)\n    for arg do\n      if [ \"$arg\" = {} ]; then\n        {} \"$@\" || exit $?\n        exit 71\n      fi\n    done;;\nesac\nexec {} \"$@\"\n",
            shell.display(), quote(verb), git, git,
        )).unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        aw_git::source_driver::GitSourceDriver::new(wrapper).unwrap()
    }
    fn write(&self, p: &str, b: &[u8]) {
        let p = self.root.join(p);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, b).unwrap();
    }
    fn entry(&self, group: ChangeGroup) -> Change {
        self.repo()
            .status()
            .unwrap()
            .changes
            .into_iter()
            .find(|c| c.group == group)
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.parent);
    }
}
#[cfg(feature = "source-driver")]
#[test]
fn prepared_git_derive_reconciles_capture_only_without_retry() {
    use aw_agent_core::{
        CheckpointPolicy, RecoveryInspection, SourceAccess, SourceName, SourceOperationKind,
    };
    use aw_git::source_driver::{GitRestoreLocation, GitSourceDriver};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let f = Fixture::new();
    let driver = GitSourceDriver::new(&f.git).unwrap();
    let binding = driver
        .binding(
            &f.root,
            SourceName::new("code").unwrap(),
            SourceAccess::ReadWrite,
            CheckpointPolicy::Required,
        )
        .unwrap();
    f.write("a.txt", b"dirty parent");
    let destination = f.parent.join("derived");
    aw_platform::ensure_private_dir(&destination).unwrap();
    let identity = |p: &std::path::Path| {
        let meta = std::fs::metadata(p).unwrap();
        (meta.dev(), meta.ino())
    };
    let location = GitRestoreLocation {
        name: binding.name.clone(),
        repository: f.root.clone(),
        repository_identity: identity(&f.root),
        target: destination.join("code"),
        parent_identity: identity(&destination),
    };
    let prepared = driver
        .plan_derive_approved(&location, &binding, "sbx_capture_only")
        .unwrap();
    assert_eq!(prepared.operation, SourceOperationKind::Derive);
    assert_eq!(prepared.descriptor.recovery_schema, 4);
    assert!(matches!(
        driver.inspect_derive(&prepared).unwrap(),
        RecoveryInspection::Unknown
    ));
    let retention = "refs/aw/checkpoints/sbx_capture_only/code";
    assert!(f.run(&["for-each-ref", retention]).stdout.is_empty());

    // Refuse only worktree creation, after the actual capture succeeds.
    // No artificial record or synthetic anchor supplied by the test.
    let shell = std::env::var_os("AW_BASH")
        .map(PathBuf::from)
        .or_else(|| option_env!("AW_DEFAULT_BASH").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let wrapper = f.parent.join("interrupt-derive");
    let executable = format!("'{}'", f.git.to_str().unwrap().replace('\'', "'\\''"));
    std::fs::write(&wrapper, format!(
        "#!{}\nprevious=\nfor arg do\n  if [ \"$previous\" = worktree ] && [ \"$arg\" = add ]; then exit 71; fi\n  previous=$arg\ndone\nexec {} \"$@\"\n",
        shell.display(), executable,
    )).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let interrupted = GitSourceDriver::new(wrapper).unwrap();
    assert!(interrupted.execute_derive(&prepared).is_err());
    assert!(!f.run(&["for-each-ref", retention]).stdout.is_empty());
    assert!(!location.target.exists());
    assert!(matches!(
        driver.inspect_derive(&prepared).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(driver.execute_derive(&prepared).is_err());
    std::fs::create_dir(&location.target).unwrap();
    assert!(driver.release_prepared_derive(&prepared).is_err());
    assert!(!f.run(&["for-each-ref", retention]).stdout.is_empty());
    std::fs::remove_dir(&location.target).unwrap();
    driver.release_prepared_derive(&prepared).unwrap();
    driver.release_prepared_derive(&prepared).unwrap();
    assert!(f.run(&["for-each-ref", retention]).stdout.is_empty());
    assert_eq!(
        std::fs::read(f.root.join("a.txt")).unwrap(),
        b"dirty parent"
    );
    assert!(!location.target.exists());
}

#[cfg(feature = "source-driver")]
#[test]
fn prepared_git_capture_retains_exact_clean_head_and_never_replays() {
    use aw_agent_core::{
        CheckpointPolicy, DriverKey, Error as SourceError, ExternalEvidence,
        PreparedSourceOperation, RecoveryInspection, SourceAccess, SourceDriverRegistry,
        SourceName,
    };
    use aw_git::source_driver::GitSourceDriver;

    let f = Fixture::new();
    let registry = SourceDriverRegistry::new();
    let handle = GitSourceDriver::new(&f.git)
        .unwrap()
        .register(&registry)
        .unwrap();
    assert_eq!(handle.driver(), &DriverKey::builtin("git").unwrap());
    let reader = registry.reader().unwrap();
    let driver = reader.get(&handle).unwrap();
    let binding = driver
        .binding(
            &f.root,
            SourceName::new("code").unwrap(),
            SourceAccess::ReadWrite,
            CheckpointPolicy::Required,
        )
        .unwrap();
    assert_eq!(
        reader.resolve(&binding.pin().unwrap()).unwrap().driver,
        binding.driver
    );

    f.write("a.txt", b"retained content\n");
    f.run(&["add", "a.txt"]);
    f.run(&["commit", "-m", "retained", "--no-gpg-sign"]);
    let index = std::fs::read(f.root.join(".git/index")).unwrap();
    let head = f.run(&["rev-parse", "HEAD"]).stdout;
    let prepared = driver
        .plan_capture(&binding, "cp_clean", "op_clean")
        .unwrap();
    let descriptor = serde_json::to_vec(&prepared).unwrap();
    let private_ref = "refs/aw/checkpoints/cp_clean/code";
    assert!(f.run(&["for-each-ref", private_ref]).stdout.is_empty());
    assert_eq!(std::fs::read(f.root.join(".git/index")).unwrap(), index);
    assert!(matches!(
        driver.inspect_capture(&prepared).unwrap(),
        RecoveryInspection::Unknown
    ));

    let anchor = driver.execute_capture(&prepared).unwrap();
    let durable = aw_agent_core::SourceAnchor::try_from(&anchor).unwrap();
    let durable: aw_agent_core::SourceAnchor =
        serde_json::from_slice(&serde_json::to_vec(&durable).unwrap()).unwrap();
    assert_eq!(
        aw_git::source_driver::GitSourceAnchor::try_from(&durable).unwrap(),
        anchor
    );
    assert_eq!(anchor.retention_ref(), private_ref);
    assert_eq!(
        anchor.captured_commit.as_bytes(),
        head.strip_suffix(b"\n").unwrap()
    );
    assert_eq!(anchor.captured_commit, anchor.base_commit);
    assert_eq!(
        reader.resolve(&anchor.pin().unwrap()).unwrap().driver,
        binding.driver
    );
    driver.revalidate_capture(&prepared, &anchor).unwrap();
    assert_eq!(f.run(&["rev-parse", "HEAD"]).stdout, head);
    assert_eq!(
        f.run(&["symbolic-ref", "HEAD"]).stdout,
        b"refs/heads/main\n"
    );
    assert_eq!(std::fs::read(f.root.join(".git/index")).unwrap(), index);

    // A schema-1 descriptor from the clean-only implementation is still
    // executable/inspectable under the new registry, with its original pin.
    let mut legacy = driver
        .plan_capture(&binding, "cp_legacy", "op_legacy")
        .unwrap();
    let d = &mut legacy.descriptor;
    d.recovery_schema = 1;
    d.fields
        .retain(|field| !matches!(field.name.as_str(), "tree" | "status"));
    {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"aw-git-capture-v1\0");
        let mut part = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        };
        part(d.prepare_id.as_bytes());
        for field in &d.fields {
            part(field.name.as_bytes());
            part(field.value.as_bytes());
        }
        d.prepared_identity = format!("{:x}", hash.finalize());
    }
    assert!(reader.resolve(&legacy.descriptor.pin().unwrap()).is_ok());
    let legacy_anchor = driver.execute_capture(&legacy).unwrap();
    assert_eq!(legacy_anchor.anchor_schema, 1);
    assert_eq!(legacy_anchor.captured_commit, anchor.captured_commit);
    driver.revalidate_capture(&legacy, &legacy_anchor).unwrap();
    assert!(
        matches!(driver.inspect_capture(&legacy).unwrap(), RecoveryInspection::Completed { result, .. } if result == legacy_anchor)
    );

    // New runtime registry, same durable identity; no re-execution.
    let restarted = SourceDriverRegistry::new();
    let handle = GitSourceDriver::new(&f.git)
        .unwrap()
        .register(&restarted)
        .unwrap();
    let driver = restarted.reader().unwrap().get(&handle).unwrap();
    let prepared: PreparedSourceOperation = serde_json::from_slice(&descriptor).unwrap();
    match driver.inspect_capture(&prepared).unwrap() {
        RecoveryInspection::Completed {
            result,
            evidence: ExternalEvidence::Immutable { .. },
        } => {
            assert_eq!(result, anchor);
        }
        result => panic!("expected exact retained capture, got {result:?}"),
    }
    assert!(driver.execute_capture(&prepared).is_err());

    // A ref without its object closure is damage, not a completed capture.
    let blob = String::from_utf8(f.run(&["rev-parse", "HEAD:a.txt"]).stdout).unwrap();
    let blob = blob.trim();
    let object = f
        .root
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    let missing = f.parent.join("missing-blob");
    std::fs::rename(&object, &missing).unwrap();
    assert!(driver.verify_anchor(&f.root, &anchor).is_err());
    assert!(matches!(
        driver.inspect_capture(&prepared).unwrap(),
        RecoveryInspection::Unknown
    ));
    std::fs::rename(&missing, &object).unwrap();

    // A branch move and ordinary GC cannot discard a live clean anchor.
    // Only this isolated fixture is reset, never the authoring repository.
    f.run(&["reset", "--hard", "HEAD^"]);
    f.run(&["reflog", "expire", "--expire=now", "--all"]);
    f.run(&["gc", "--prune=now"]);
    driver.verify_anchor(&f.root, &anchor).unwrap();
    assert!(driver.revalidate_capture(&prepared, &anchor).is_err());
    assert!(matches!(
        driver.inspect_capture(&prepared).unwrap(),
        RecoveryInspection::Completed { .. }
    ));

    let stale = driver
        .plan_capture(&binding, "cp_stale", "op_stale")
        .unwrap();
    f.write("a.txt", b"dirty after plan\n");
    assert!(matches!(
        driver.execute_capture(&stale),
        Err(SourceError::Conflict(_))
    ));
    assert!(driver
        .plan_capture(&binding, "cp_dirty", "op_dirty")
        .is_ok());
    assert!(f
        .run(&["for-each-ref", "refs/aw/checkpoints/cp_stale"])
        .stdout
        .is_empty());
    f.run(&["restore", "a.txt"]);

    // Index flags cannot manufacture an apparently clean source.
    f.run(&["update-index", "--assume-unchanged", "a.txt"]);
    assert!(driver
        .plan_capture(&binding, "cp_hidden", "op_hidden")
        .is_err());
    f.run(&["update-index", "--no-assume-unchanged", "a.txt"]);
    f.run(&["config", "filter.unsafe.smudge", "false"]);
    assert!(driver
        .plan_capture(&binding, "cp_filter", "op_filter")
        .is_err());
    f.run(&["config", "--unset", "filter.unsafe.smudge"]);
    f.run(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{},nested", anchor.captured_commit),
    ]);
    // A staged-only gitlink whose worktree path is absent is a deletion
    // in the final tree, just like a staged regular file removed afterwards.
    assert!(driver
        .plan_capture(&binding, "cp_gitlink", "op_gitlink")
        .is_ok());
    f.run(&["update-index", "--force-remove", "nested"]);
    f.write(".git/objects/info/alternates", b"\n");
    assert!(driver
        .plan_capture(&binding, "cp_alternate", "op_alternate")
        .is_err());
    std::fs::remove_file(f.root.join(".git/objects/info/alternates")).unwrap();

    let collision = driver
        .plan_capture(&binding, "cp_collision", "op_collision")
        .unwrap();
    let wrong_ref = "refs/aw/checkpoints/cp_collision/code";
    f.run(&["update-ref", wrong_ref, &anchor.captured_commit]);
    assert!(driver.execute_capture(&collision).is_err());
    assert!(matches!(
        driver.inspect_capture(&collision).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert_eq!(f.run(&["rev-parse", wrong_ref]).stdout, head);
    assert!(driver.release_prepared_capture(&collision).is_err());
    assert_eq!(f.run(&["rev-parse", wrong_ref]).stdout, head);
    driver.release_prepared_capture(&stale).unwrap();

    // Unpublished dirty capture retirement reuses the exact original
    // descriptor. A leftover scratch directory is not proof of ownership.
    f.write("a.txt", b"abandoned dirty capture\n");
    let abandoned = driver
        .plan_capture(&binding, "cp_abandoned", "op_abandoned")
        .unwrap();
    let retained = driver.execute_capture(&abandoned).unwrap();
    let scratch = f.root.join(".git").join(format!(
        "aw-capture-{}",
        abandoned.descriptor.prepared_identity
    ));
    std::fs::create_dir(&scratch).unwrap();
    assert!(driver.release_prepared_capture(&abandoned).is_err());
    driver.verify_anchor(&f.root, &retained).unwrap();
    std::fs::remove_dir(&scratch).unwrap();
    driver.release_prepared_capture(&abandoned).unwrap();
    driver.release_prepared_capture(&abandoned).unwrap();
    assert!(driver.verify_anchor(&f.root, &retained).is_err());
    assert_eq!(
        std::fs::read(f.root.join("a.txt")).unwrap(),
        b"abandoned dirty capture\n"
    );
    // Interrupt the real private-index pass after Git has written it.
    // The original descriptor and the driver's synced marker now identify
    // only the scratch directory, never the real checkout/index.
    let partial = driver
        .plan_capture(&binding, "cp_partial_index", "op_partial_index")
        .unwrap();
    let before_index = std::fs::read(f.root.join(".git/index")).unwrap();
    let before_head = f.run(&["rev-parse", "HEAD"]).stdout;
    assert!(f
        .interrupt_private_index("read-tree")
        .execute_capture(&partial)
        .is_err());
    let name = format!("aw-capture-{}", partial.descriptor.prepared_identity);
    let scratch = f.root.join(".git").join(&name);
    let marker = f.root.join(".git").join(format!("{name}.owner"));
    assert!(scratch.join("index-1").is_file() && marker.is_file());
    assert!(matches!(
        driver.inspect_capture(&partial).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(driver.execute_capture(&partial).is_err());

    // Same private mode and derived name with the original sibling
    // marker are insufficient when the directory identity differs.
    use std::os::unix::fs::DirBuilderExt;
    let moved = scratch.with_extension("moved");
    std::fs::rename(&scratch, &moved).unwrap();
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&scratch)
        .unwrap();
    std::fs::write(scratch.join("index-1"), b"foreign").unwrap();
    assert!(driver.release_prepared_capture(&partial).is_err());
    assert_eq!(std::fs::read(scratch.join("index-1")).unwrap(), b"foreign");
    std::fs::remove_file(scratch.join("index-1")).unwrap();
    std::fs::remove_dir(&scratch).unwrap();
    std::fs::rename(&moved, &scratch).unwrap();
    std::fs::write(scratch.join("unexpected"), b"keep").unwrap();
    assert!(driver.release_prepared_capture(&partial).is_err());
    assert!(scratch.join("index-1").is_file());
    assert_eq!(std::fs::read(scratch.join("unexpected")).unwrap(), b"keep");
    std::fs::remove_file(scratch.join("unexpected")).unwrap();
    driver.release_prepared_capture(&partial).unwrap();
    assert!(!scratch.exists() && !marker.exists());
    driver.release_prepared_capture(&partial).unwrap();
    assert_eq!(
        std::fs::read(f.root.join(".git/index")).unwrap(),
        before_index
    );
    assert_eq!(f.run(&["rev-parse", "HEAD"]).stdout, before_head);
    assert_eq!(
        std::fs::read(f.root.join("a.txt")).unwrap(),
        b"abandoned dirty capture\n"
    );
    f.run(&["restore", "a.txt"]);

    // Retirement owns an exact conditional release, never a broad ref
    // deletion. A changed or symbolic ref must remain untouched.
    let current_head = String::from_utf8(f.run(&["rev-parse", "HEAD"]).stdout).unwrap();
    let current_head = current_head.trim();
    let current_index = std::fs::read(f.root.join(".git/index")).unwrap();
    f.run(&["update-ref", private_ref, current_head]);
    assert!(driver.release_retired_anchor(&f.root, &anchor).is_err());
    assert_eq!(
        f.run(&["rev-parse", private_ref]).stdout,
        format!("{current_head}\n").as_bytes()
    );
    f.run(&["symbolic-ref", private_ref, "refs/heads/main"]);
    assert!(driver.release_retired_anchor(&f.root, &anchor).is_err());
    assert_eq!(
        f.run(&["symbolic-ref", private_ref]).stdout,
        b"refs/heads/main\n"
    );
    f.run(&[
        "update-ref",
        "--no-deref",
        private_ref,
        &anchor.captured_commit,
    ]);
    driver.release_retired_anchor(&f.root, &anchor).unwrap();
    driver.release_retired_anchor(&f.root, &anchor).unwrap();
    assert_eq!(
        f.run(&["rev-parse", "HEAD"]).stdout,
        format!("{current_head}\n").as_bytes()
    );
    assert_eq!(
        std::fs::read(f.root.join(".git/index")).unwrap(),
        current_index
    );
    driver.verify_anchor(&f.root, &legacy_anchor).unwrap();
    assert!(f.run(&["for-each-ref", private_ref]).stdout.is_empty());

    // Deleting the retention fact is uncertainty for capture, not a
    // fresh capture operation.
    assert!(matches!(
        driver.inspect_capture(&prepared).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(driver.verify_anchor(&f.root, &anchor).is_err());

    // Same pathname, new checkout/object directory: do not adopt it.
    std::fs::rename(&f.root, f.parent.join("original")).unwrap();
    std::fs::create_dir(&f.root).unwrap();
    f.run(&["init", "--initial-branch=main"]);
    assert!(driver
        .plan_capture(&binding, "cp_replaced", "op_replaced")
        .is_err());
    assert!(driver.release_retired_anchor(&f.root, &anchor).is_err());
    assert!(matches!(
        driver.inspect_capture(&collision).unwrap(),
        RecoveryInspection::Unknown
    ));
}

#[cfg(feature = "source-driver")]
#[test]
fn prepared_git_capture_preserves_final_dirty_tree_and_exact_gitlinks() {
    use aw_agent_core::{
        CheckpointPolicy, PreparedSourceOperation, RecoveryInspection, SourceAccess, SourceName,
    };
    use aw_git::source_driver::GitSourceDriver;
    use std::os::unix::fs::{symlink, PermissionsExt};

    let f = Fixture::new();
    let module = Fixture::new();
    f.run(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        module.root.to_str().unwrap(),
        "nested",
    ]);
    f.write(".gitignore", b"ignored*\n");
    f.write(".gitattributes", b"*.txt text eol=lf\n");
    f.write("deleted", b"delete me");
    f.write("dir/old", b"old child");
    symlink("a.txt", f.root.join("tracked-link")).unwrap();
    f.run(&["add", "."]);
    f.run(&["commit", "-m", "base with gitlink", "--no-gpg-sign"]);
    let base = String::from_utf8(f.run(&["rev-parse", "HEAD"]).stdout).unwrap();
    let base = base.trim();
    let old_gitlink = String::from_utf8(f.run(&["rev-parse", "HEAD:nested"]).stdout).unwrap();

    let driver = GitSourceDriver::new(&f.git).unwrap();
    let binding = driver
        .binding(
            &f.root,
            SourceName::new("code").unwrap(),
            SourceAccess::ReadWrite,
            CheckpointPolicy::Required,
        )
        .unwrap();

    f.write("a.txt", b"staged\n");
    f.run(&["add", "a.txt"]);
    f.write("a.txt", b"working\r\n");
    std::fs::remove_file(f.root.join("deleted")).unwrap();
    std::fs::remove_dir_all(f.root.join("dir")).unwrap();
    f.write("dir", b"replacement file");
    f.write("ignored-untracked", b"must not enter snapshot");
    f.write("ignored-staged", b"first staged bytes");
    f.run(&["add", "-f", "ignored-staged"]);
    f.write("ignored-staged", b"final tracked bytes");
    f.write("new\nfile", b"untracked regular bytes\0");
    f.write("script", b"#!/bin/sh\n");
    std::fs::set_permissions(
        f.root.join("script"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::remove_file(f.root.join("tracked-link")).unwrap();
    symlink("deleted", f.root.join("tracked-link")).unwrap();
    symlink(&module.root, f.root.join("untracked-link")).unwrap();

    // Dirty nested repositories are refused, not traversed into the parent tree.
    f.write("nested/a.txt", b"dirty nested\n");
    assert!(driver
        .plan_capture(&binding, "cp_nested_dirty", "op_nested_dirty")
        .is_err());
    let nested = |args: &[&str]| {
        let out = f
            .cmd()
            .current_dir(f.root.join("nested"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "nested git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    };
    nested(&["add", "a.txt"]);
    nested(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@aw.invalid",
        "commit",
        "-m",
        "new nested HEAD",
        "--no-gpg-sign",
    ]);
    let nested_head = String::from_utf8(nested(&["rev-parse", "HEAD"])).unwrap();

    let index = std::fs::read(f.root.join(".git/index")).unwrap();
    let objects = f.run(&["count-objects", "-v"]).stdout;
    let prepared = driver
        .plan_capture(&binding, "cp_dirty_tree", "op_dirty_tree")
        .unwrap();
    assert_eq!(f.run(&["count-objects", "-v"]).stdout, objects);
    assert!(f
        .run(&["for-each-ref", "refs/aw/checkpoints/cp_dirty_tree"])
        .stdout
        .is_empty());
    assert_eq!(std::fs::read(f.root.join(".git/index")).unwrap(), index);
    assert!(matches!(
        driver.inspect_capture(&prepared).unwrap(),
        RecoveryInspection::Unknown
    ));

    let anchor = driver.execute_capture(&prepared).unwrap();
    let durable = aw_agent_core::SourceAnchor::try_from(&anchor).unwrap();
    assert_eq!(
        aw_git::source_driver::GitSourceAnchor::try_from(&durable).unwrap(),
        anchor
    );
    assert!(anchor.is_dirty());
    assert_eq!(anchor.base_commit, base);
    assert_eq!(
        f.run(&["rev-parse", "HEAD"]).stdout,
        format!("{base}\n").as_bytes()
    );
    assert_eq!(
        f.run(&["symbolic-ref", "HEAD"]).stdout,
        b"refs/heads/main\n"
    );
    assert_eq!(std::fs::read(f.root.join(".git/index")).unwrap(), index);
    let contents = |path: &str| {
        f.run(&["show", &format!("{}:{path}", anchor.captured_commit)])
            .stdout
    };
    assert_eq!(contents("a.txt"), b"working\n");
    assert_eq!(contents("dir"), b"replacement file");
    assert_eq!(contents("ignored-staged"), b"final tracked bytes");
    assert_eq!(contents("new\nfile"), b"untracked regular bytes\0");
    assert_eq!(contents("tracked-link"), b"deleted");
    let tree = f
        .run(&["ls-tree", "-r", "-z", &anchor.captured_commit])
        .stdout;
    let paths: Vec<_> = tree
        .split(|b| *b == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.splitn(2, |b| *b == b'\t').nth(1).unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            b".gitattributes".as_slice(),
            b".gitignore",
            b".gitmodules",
            b"a.txt",
            b"dir",
            b"ignored-staged",
            b"nested",
            b"new\nfile",
            b"script",
            b"tracked-link",
        ]
    );
    assert!(tree
        .split(|b| *b == 0)
        .any(|entry| entry.starts_with(b"100755 blob ") && entry.ends_with(b"\tscript")));
    assert!(tree
        .split(|b| *b == 0)
        .any(|entry| entry.starts_with(b"120000 blob ") && entry.ends_with(b"\ttracked-link")));
    assert_eq!(
        f.run(&["rev-parse", &format!("{}:nested", anchor.captured_commit)])
            .stdout,
        nested_head.as_bytes()
    );
    driver.revalidate_capture(&prepared, &anchor).unwrap();

    let stale = driver
        .plan_capture(&binding, "cp_stale_dirty", "op_stale_dirty")
        .unwrap();
    // Same status, different content must fail the observation fence.
    f.write("a.txt", b"changed\r\n");
    assert!(driver.execute_capture(&stale).is_err());
    assert!(driver.revalidate_capture(&prepared, &anchor).is_err());
    let descriptor: PreparedSourceOperation =
        serde_json::from_slice(&serde_json::to_vec(&prepared).unwrap()).unwrap();
    let restarted = GitSourceDriver::new(&f.git).unwrap();
    match restarted.inspect_capture(&descriptor).unwrap() {
        RecoveryInspection::Completed { result, .. } => assert_eq!(result, anchor),
        other => panic!("expected retained dirty capture, got {other:?}"),
    }

    // An unrelated commit with the very same tree and parent is not this
    // Prepared operation, even if the private ref is redirected to it.
    let tree_oid = String::from_utf8(
        f.run(&["rev-parse", &format!("{}^{{tree}}", anchor.captured_commit)])
            .stdout,
    )
    .unwrap();
    let other = String::from_utf8(
        f.run(&[
            "commit-tree",
            tree_oid.trim(),
            "-p",
            base,
            "-m",
            "not this capture",
            "--no-gpg-sign",
        ])
        .stdout,
    )
    .unwrap();
    f.run(&["update-ref", &anchor.retention_ref(), other.trim()]);
    assert!(matches!(
        restarted.inspect_capture(&descriptor).unwrap(),
        RecoveryInspection::Unknown
    ));
    f.run(&[
        "update-ref",
        &anchor.retention_ref(),
        &anchor.captured_commit,
    ]);

    // An uninitialized empty gitlink retains the index OID, without fetching
    // or copying a nested repository's files into the superproject.
    f.run(&["submodule", "deinit", "-f", "--", "nested"]);
    let uninitialized = driver
        .plan_capture(&binding, "cp_uninitialized", "op_uninitialized")
        .unwrap();
    let uninitialized = driver.execute_capture(&uninitialized).unwrap();
    assert_eq!(
        f.run(&[
            "rev-parse",
            &format!("{}:nested", uninitialized.captured_commit)
        ])
        .stdout,
        old_gitlink.as_bytes()
    );

    // A live synthetic retention ref protects both trees after the user
    // moves HEAD and ordinary Git GC prunes unretained objects.
    f.run(&["reset", "--hard", "HEAD^"]);
    f.run(&["reflog", "expire", "--expire=now", "--all"]);
    f.run(&["gc", "--prune=now"]);
    restarted.verify_anchor(&f.root, &anchor).unwrap();
    assert_eq!(contents("a.txt"), b"working\n");
    assert_eq!(
        f.run(&["rev-parse", &format!("{}^", anchor.captured_commit)])
            .stdout,
        format!("{base}\n").as_bytes()
    );
}

#[cfg(feature = "source-driver")]
#[test]
fn prepared_git_restore_materializes_exact_tree_without_exposing_synthetic_head() {
    use aw_agent_core::{
        CheckpointPolicy, ExternalEvidence, PreparedSourceOperation, RecoveryInspection,
        SourceAccess, SourceDriverRegistry, SourceName,
    };
    use aw_git::source_driver::GitSourceDriver;
    use std::os::unix::fs::{symlink, PermissionsExt};

    let f = Fixture::new();
    let base_link = String::from_utf8(f.run(&["rev-parse", "HEAD"]).stdout).unwrap();
    let base_link = base_link.trim();
    f.write(".gitignore", b"ignored*\n");
    f.write(".gitattributes", b"*.txt text eol=lf\n");
    f.write("remove.txt", b"remove\n");
    f.write("replace/child", b"child");
    symlink("a.txt", f.root.join("base-link")).unwrap();
    f.run(&["add", "."]);
    f.run(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{base_link},nested"),
    ]);
    std::fs::create_dir(f.root.join("nested")).unwrap();
    f.run(&["commit", "-m", "base", "--no-gpg-sign"]);
    let base = String::from_utf8(f.run(&["rev-parse", "HEAD"]).stdout).unwrap();
    let base = base.trim();
    let registry = SourceDriverRegistry::new();
    let handle = GitSourceDriver::new(&f.git)
        .unwrap()
        .register(&registry)
        .unwrap();
    let reader = registry.reader().unwrap();
    let driver = reader.get(&handle).unwrap();
    let source = driver
        .binding(
            &f.root,
            SourceName::new("code").unwrap(),
            SourceAccess::ReadOnly,
            CheckpointPolicy::Ephemeral,
        )
        .unwrap();
    let clean = driver
        .plan_capture(&source, "cp_restore_clean", "op_capture_clean")
        .unwrap();
    let clean = driver.execute_capture(&clean).unwrap();
    assert!(!clean.is_dirty());

    f.write("a.txt", b"staged\n");
    f.run(&["add", "a.txt"]);
    f.write("a.txt", b"final\r\n");
    std::fs::remove_file(f.root.join("remove.txt")).unwrap();
    std::fs::remove_dir_all(f.root.join("replace")).unwrap();
    f.write("replace", b"regular file");
    f.write("ignored-retained", b"staged ignored file");
    f.run(&["add", "-f", "ignored-retained"]);
    f.write("ignored-retained", b"final ignored file");
    f.write("ignored-excluded", b"must not restore");
    symlink("missing-target", f.root.join("added-link")).unwrap();
    f.run(&["add", "added-link"]);
    symlink("outside", f.root.join("excluded-link")).unwrap();
    f.write("new\nfile", b"new bytes\0");
    f.write("script", b"#!/bin/sh\n");
    std::fs::set_permissions(
        f.root.join("script"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let dirty = driver
        .plan_capture(&source, "cp_restore_dirty", "op_capture_dirty")
        .unwrap();
    let dirty = driver.execute_capture(&dirty).unwrap();
    assert!(dirty.is_dirty());

    // Move the live source on. Neither restore is tied to its current HEAD.
    f.run(&["reset", "--hard", "HEAD^"]);
    f.write("a.txt", b"unrelated current source\n");
    let original_head = f.run(&["rev-parse", "HEAD"]).stdout;
    let original_index = std::fs::read(f.root.join(".git/index")).unwrap();
    let original_worktrees = f.run(&["worktree", "list", "--porcelain", "-z"]).stdout;
    let destinations = f.parent.join("restores");
    aw_platform::ensure_private_dir(&destinations).unwrap();
    let clean_path = destinations.join("clean");
    let dirty_path = destinations.join("dirty");
    assert!(driver
        .plan_restore(&f.root, &clean, &f.root.join("inside"), "op_overlap")
        .is_err());
    let clean_plan = driver
        .plan_restore(&f.root, &clean, &clean_path, "op_restore_clean")
        .unwrap();
    let dirty_plan = driver
        .plan_restore(&f.root, &dirty, &dirty_path, "op_restore_dirty")
        .unwrap();
    assert_eq!(dirty_plan.descriptor.recovery_schema, 3);
    assert!(reader
        .resolve(&dirty_plan.descriptor.pin().unwrap())
        .is_ok());
    assert_eq!(
        f.run(&["worktree", "list", "--porcelain", "-z"]).stdout,
        original_worktrees
    );
    assert!(!clean_path.exists() && !dirty_path.exists());
    assert!(matches!(
        driver.inspect_restore(&dirty_plan).unwrap(),
        RecoveryInspection::Unknown
    ));

    let restored_clean = driver.execute_restore(&clean_plan).unwrap();
    let restored_dirty = driver.execute_restore(&dirty_plan).unwrap();
    assert_eq!(restored_dirty.source_ref, source.source_ref);
    assert_ne!(restored_dirty.materialization, source.materialization);
    assert_ne!(
        restored_dirty.materialization,
        restored_clean.materialization
    );
    assert_eq!(restored_dirty.name, source.name);
    assert_eq!(restored_dirty.access, SourceAccess::ReadOnly);
    assert_eq!(
        restored_dirty.checkpoint_policy,
        CheckpointPolicy::Ephemeral
    );

    let child_git = |path: &std::path::Path, args: &[&str]| {
        let output = f.cmd().current_dir(path).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    for path in [&clean_path, &dirty_path] {
        assert_eq!(
            child_git(path, &["rev-parse", "HEAD"]),
            format!("{base}\n").as_bytes()
        );
        assert!(child_git(path, &["diff", "--cached", "--name-only"]).is_empty());
        assert!(std::fs::read_dir(path.join("nested"))
            .unwrap()
            .next()
            .is_none());
        assert!(!path.join("nested/.git").exists());
    }
    assert_eq!(
        std::fs::read(clean_path.join("a.txt")).unwrap(),
        b"one\ntwo\n"
    );
    assert!(child_git(&clean_path, &["status", "--porcelain", "-z"]).is_empty());
    assert_eq!(std::fs::read(dirty_path.join("a.txt")).unwrap(), b"final\n");
    assert!(!dirty_path.join("remove.txt").exists());
    assert_eq!(
        std::fs::read(dirty_path.join("replace")).unwrap(),
        b"regular file"
    );
    assert_eq!(
        std::fs::read(dirty_path.join("ignored-retained")).unwrap(),
        b"final ignored file"
    );
    assert_eq!(
        std::fs::read(dirty_path.join("new\nfile")).unwrap(),
        b"new bytes\0"
    );
    assert_eq!(
        std::fs::read_link(dirty_path.join("added-link")).unwrap(),
        std::path::Path::new("missing-target")
    );
    assert_eq!(
        std::fs::read_link(dirty_path.join("base-link")).unwrap(),
        std::path::Path::new("a.txt")
    );
    assert_eq!(
        std::fs::metadata(dirty_path.join("script"))
            .unwrap()
            .permissions()
            .mode()
            & 0o100,
        0o100
    );
    assert!(!dirty_path.join("ignored-excluded").exists());
    assert!(!dirty_path.join("excluded-link").is_symlink());
    let status = child_git(&dirty_path, &["status", "--porcelain", "-z"]);
    assert!(status.windows(7).any(|b| b == b" M a.tx"));
    assert!(status.windows(6).any(|b| b == b"?? new"));

    // Exact Prepared descriptor and observed result survive a new driver.
    let dirty_plan: PreparedSourceOperation =
        serde_json::from_slice(&serde_json::to_vec(&dirty_plan).unwrap()).unwrap();
    let restarted = GitSourceDriver::new(&f.git).unwrap();
    match restarted.inspect_restore(&dirty_plan).unwrap() {
        RecoveryInspection::Completed {
            result,
            evidence: ExternalEvidence::HeldFence { handle, .. },
        } => {
            assert_eq!(result, restored_dirty);
            assert_eq!(handle, dirty_plan.descriptor.prepared_identity);
        }
        other => panic!("expected exact observed restore, got {other:?}"),
    }
    restarted
        .revalidate_restore(&dirty_plan, &restored_dirty)
        .unwrap();
    restarted
        .revalidate_restore(&clean_plan, &restored_clean)
        .unwrap();
    assert!(restarted.execute_restore(&dirty_plan).is_err());

    // The exact published binding can realize a mutable checkout after
    // restore, but neither another root nor a repointed Git administration
    // directory can inherit its identity.
    let approved = aw_platform::DirectoryIdentity::open(&dirty_path).unwrap();
    let physical = approved.identity().unwrap();
    let verified = restarted
        .verify_materialization(&restored_dirty, &dirty_path, physical)
        .unwrap();
    assert_eq!(verified.root(), dirty_path);
    assert_eq!(verified.root_identity().unwrap(), physical);
    assert!(restarted
        .verify_materialization(&restored_dirty, &clean_path, physical)
        .is_err());
    let other = aw_platform::DirectoryIdentity::open(&clean_path)
        .unwrap()
        .identity()
        .unwrap();
    assert!(restarted
        .verify_materialization(&restored_dirty, &dirty_path, other)
        .is_err());
    let pointer = std::fs::read(dirty_path.join(".git")).unwrap();
    std::fs::write(
        dirty_path.join(".git"),
        std::fs::read(clean_path.join(".git")).unwrap(),
    )
    .unwrap();
    assert!(restarted
        .verify_materialization(&restored_dirty, &dirty_path, physical)
        .is_err());
    std::fs::write(dirty_path.join(".git"), pointer).unwrap();
    drop(verified);

    // Recovery cannot mistake extra ignored content for a completed restore.
    std::fs::write(dirty_path.join("ignored-extra"), b"foreign content").unwrap();
    assert!(matches!(
        restarted.inspect_restore(&dirty_plan).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(restarted
        .revalidate_restore(&dirty_plan, &restored_dirty)
        .is_err());
    assert!(restarted
        .release_restored_materialization(&dirty_plan, &restored_dirty)
        .is_err());
    assert!(dirty_path.join("ignored-extra").is_file());
    std::fs::remove_file(dirty_path.join("ignored-extra")).unwrap();
    std::fs::create_dir(dirty_path.join("foreign-empty")).unwrap();
    assert!(matches!(
        restarted.inspect_restore(&dirty_plan).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(restarted
        .release_restored_materialization(&dirty_plan, &restored_dirty)
        .is_err());
    assert!(dirty_path.join("foreign-empty").is_dir());
    std::fs::remove_dir(dirty_path.join("foreign-empty")).unwrap();
    // Published materialization retirement requires the complete tree,
    // not merely a subset of its original content.
    let leaf = dirty_path.join("new\nfile");
    std::fs::remove_file(&leaf).unwrap();
    assert!(restarted
        .release_restored_materialization(&dirty_plan, &restored_dirty)
        .is_err());
    std::fs::write(&leaf, b"new bytes\0").unwrap();
    child_git(&dirty_path, &["read-tree", &dirty.captured_commit]);
    assert!(matches!(
        restarted.inspect_restore(&dirty_plan).unwrap(),
        RecoveryInspection::Unknown
    ));
    child_git(&dirty_path, &["read-tree", base]);
    f.run(&["worktree", "unlock", dirty_path.to_str().unwrap()]);
    f.run(&[
        "worktree",
        "lock",
        "--reason",
        "another-operation",
        dirty_path.to_str().unwrap(),
    ]);
    assert!(matches!(
        restarted.inspect_restore(&dirty_plan).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(restarted.execute_restore(&dirty_plan).is_err());
    assert!(restarted
        .release_restored_materialization(&dirty_plan, &restored_dirty)
        .is_err());
    assert!(dirty_path.is_dir());
    f.run(&["worktree", "unlock", dirty_path.to_str().unwrap()]);
    f.run(&[
        "worktree",
        "lock",
        "--reason",
        &format!("aw-restore-v1:{}", dirty_plan.descriptor.prepared_identity),
        dirty_path.to_str().unwrap(),
    ]);
    assert!(restarted
        .release_restored_materialization(&dirty_plan, &restored_clean)
        .is_err());

    // A crash after registration, before index/checkout, remains Unknown.
    let partial_path = destinations.join("partial");
    let partial = driver
        .plan_restore(&f.root, &dirty, &partial_path, "op_restore_partial")
        .unwrap();
    f.run(&[
        "worktree",
        "add",
        "--detach",
        "--no-checkout",
        "--lock",
        "--reason",
        &format!("aw-restore-v1:{}", partial.descriptor.prepared_identity),
        partial_path.to_str().unwrap(),
        base,
    ]);
    assert!(matches!(
        restarted.inspect_restore(&partial).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(restarted.execute_restore(&partial).is_err());
    assert_eq!(std::fs::read_dir(&partial_path).unwrap().count(), 1);
    std::fs::write(partial_path.join("foreign"), b"keep").unwrap();
    assert!(restarted.release_prepared_restore(&partial).is_err());
    assert_eq!(
        std::fs::read(partial_path.join("foreign")).unwrap(),
        b"keep"
    );
    std::fs::remove_file(partial_path.join("foreign")).unwrap();
    // The exact base index can survive before any checkout entry is
    // written. A partial private candidate can contain only the captured
    // tree's unchanged leaves and expected ancestor directories.
    child_git(&partial_path, &["read-tree", base]);
    let partial_repo = Repository::discover(&f.git, &partial_path).unwrap();
    let private = partial_repo.git_dir().join("fixture-checkout-index");
    let output = Command::new(&f.git)
        .current_dir(&partial_path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_INDEX_FILE", &private)
        .args(["read-tree", &dirty.captured_commit])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = Command::new(&f.git)
        .current_dir(&partial_path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_INDEX_FILE", &private)
        .args(["checkout-index", "--", "a.txt", "added-link"])
        .output()
        .unwrap();
    assert!(output.status.success());
    std::fs::remove_file(private).unwrap();
    let captured_leaf = std::fs::read(partial_path.join("a.txt")).unwrap();
    assert_eq!(
        std::fs::read_link(partial_path.join("added-link")).unwrap(),
        std::path::Path::new("missing-target")
    );
    assert!(matches!(
        restarted.inspect_restore(&partial).unwrap(),
        RecoveryInspection::Unknown
    ));
    assert!(restarted.execute_restore(&partial).is_err());
    std::fs::create_dir(partial_path.join("foreign-empty")).unwrap();
    assert!(restarted.release_prepared_restore(&partial).is_err());
    assert!(partial_path.join("foreign-empty").is_dir());
    std::fs::remove_dir(partial_path.join("foreign-empty")).unwrap();
    std::fs::write(
        partial_path.join("a.txt"),
        b"changed after partial checkout",
    )
    .unwrap();
    assert!(restarted.release_prepared_restore(&partial).is_err());
    assert_eq!(
        std::fs::read(partial_path.join("a.txt")).unwrap(),
        b"changed after partial checkout"
    );
    std::fs::write(partial_path.join("a.txt"), captured_leaf).unwrap();
    restarted.release_prepared_restore(&partial).unwrap();
    restarted.release_prepared_restore(&partial).unwrap();
    assert!(!partial_path.exists());

    let abandoned_path = destinations.join("abandoned");
    let abandoned = driver
        .plan_restore(&f.root, &dirty, &abandoned_path, "op_abandoned_restore")
        .unwrap();
    // Complete the real dirty checkout but lose its response before
    // scratch cleanup. Inspection must not publish over that artifact;
    // abandonment reuses exact scratch and original restore validation.
    assert!(f
        .interrupt_private_index("checkout-index")
        .execute_restore(&abandoned)
        .is_err());
    let child = Repository::discover(&f.git, &abandoned_path).unwrap();
    let name = format!("aw-capture-{}", abandoned.descriptor.prepared_identity);
    assert!(child.git_dir().join(&name).join("index-1").is_file());
    assert!(child.git_dir().join(format!("{name}.owner")).is_file());
    assert!(matches!(
        restarted.inspect_restore(&abandoned).unwrap(),
        RecoveryInspection::Unknown
    ));
    restarted.release_prepared_restore(&abandoned).unwrap();
    restarted.release_prepared_restore(&abandoned).unwrap();
    assert!(!abandoned_path.exists());

    // Existing destination, or a substituted private parent, is not reusable.
    assert!(driver
        .plan_restore(&f.root, &dirty, &dirty_path, "op_existing")
        .is_err());
    let parent = f.parent.join("replace-parent");
    aw_platform::ensure_private_dir(&parent).unwrap();
    let stale_path = parent.join("target");
    let stale = driver
        .plan_restore(&f.root, &clean, &stale_path, "op_stale_parent")
        .unwrap();
    std::fs::rename(&parent, f.parent.join("old-parent")).unwrap();
    aw_platform::ensure_private_dir(&parent).unwrap();
    assert!(restarted.execute_restore(&stale).is_err());
    assert!(!stale_path.exists());

    // A changed gitlink cannot be represented by an empty directory with the
    // base index. Refuse it before creating any new worktree.
    let link_source = destinations.join("link-source");
    let link_plan = driver
        .plan_restore(&f.root, &clean, &link_source, "op_link_source")
        .unwrap();
    driver.execute_restore(&link_plan).unwrap();
    child_git(
        &link_source,
        &[
            "update-index",
            "--cacheinfo",
            &format!("160000,{},nested", dirty.captured_commit),
        ],
    );
    let link_binding = driver
        .binding(
            &link_source,
            SourceName::new("code").unwrap(),
            SourceAccess::ReadWrite,
            CheckpointPolicy::Required,
        )
        .unwrap();
    let changed = driver
        .plan_capture(&link_binding, "cp_changed_link", "op_changed_link")
        .unwrap();
    let changed = driver.execute_capture(&changed).unwrap();
    let missing = destinations.join("requires-nested");
    assert!(driver
        .plan_restore(&f.root, &changed, &missing, "op_missing_nested")
        .is_err());
    assert!(!missing.exists());

    // Retired cleanup uses the original exact published checkout even
    // after checkpoint retention has been released. It cannot discard
    // post-restore edits, repointed checkouts or a different binding.
    std::fs::write(clean_path.join("a.txt"), b"later edit").unwrap();
    assert!(restarted
        .release_restored_materialization(&clean_plan, &restored_clean)
        .is_err());
    assert_eq!(
        std::fs::read(clean_path.join("a.txt")).unwrap(),
        b"later edit"
    );
    std::fs::write(clean_path.join("a.txt"), b"one\ntwo\n").unwrap();
    let clean_repo = aw_git::Repository::discover(&f.git, &clean_path).unwrap();
    let gitdir = clean_repo.git_dir().join("gitdir");
    let registration = std::fs::read(&gitdir).unwrap();
    drop(clean_repo);
    let moved = destinations.join("moved-clean");
    std::fs::rename(&clean_path, &moved).unwrap();
    // Registration survives, so missing target alone is not proof.
    assert!(restarted
        .release_restored_materialization(&clean_plan, &restored_clean)
        .is_err());
    std::fs::create_dir(&clean_path).unwrap();
    assert!(restarted
        .release_restored_materialization(&clean_plan, &restored_clean)
        .is_err());
    std::fs::remove_dir(&clean_path).unwrap();
    // Repoint registration too: old path and old registration name are
    // gone, but the original operation still owns this worktree.
    std::fs::write(&gitdir, format!("{}\n", moved.join(".git").display())).unwrap();
    assert!(restarted
        .release_restored_materialization(&clean_plan, &restored_clean)
        .is_err());
    assert!(moved.is_dir());
    std::fs::write(&gitdir, registration).unwrap();
    std::fs::rename(&moved, &clean_path).unwrap();
    restarted.release_retired_anchor(&f.root, &clean).unwrap();
    restarted.release_retired_anchor(&f.root, &dirty).unwrap();
    assert!(restarted.verify_anchor(&f.root, &clean).is_err());
    restarted
        .release_restored_materialization(&clean_plan, &restored_clean)
        .unwrap();
    restarted
        .release_restored_materialization(&dirty_plan, &restored_dirty)
        .unwrap();
    restarted
        .release_restored_materialization(&dirty_plan, &restored_dirty)
        .unwrap();
    assert!(!clean_path.exists() && !dirty_path.exists());
    assert!(link_source.is_dir() && !partial_path.exists());
    // Prior partial abandonment remains replayable after anchor retirement.
    restarted.release_prepared_restore(&partial).unwrap();
    restarted.release_prepared_restore(&partial).unwrap();
    assert!(!partial_path.exists());
    assert!(restarted.release_prepared_restore(&link_plan).is_err());
    assert!(link_source.is_dir());
    let remaining = f.run(&["worktree", "list", "--porcelain", "-z"]).stdout;
    for path in [&clean_path, &dirty_path] {
        assert!(!remaining
            .windows(path.to_str().unwrap().len())
            .any(|part| part == path.to_str().unwrap().as_bytes()));
    }
    // A new occupant at the retired name must not be removed by replay.
    std::fs::create_dir(&dirty_path).unwrap();
    std::fs::write(dirty_path.join("unrelated"), b"keep").unwrap();
    assert!(restarted
        .release_restored_materialization(&dirty_plan, &restored_dirty)
        .is_err());
    assert_eq!(
        std::fs::read(dirty_path.join("unrelated")).unwrap(),
        b"keep"
    );

    assert_eq!(f.run(&["rev-parse", "HEAD"]).stdout, original_head);
    assert_eq!(
        std::fs::read(f.root.join(".git/index")).unwrap(),
        original_index
    );
    assert_eq!(
        std::fs::read(f.root.join("a.txt")).unwrap(),
        b"unrelated current source\n"
    );
}

#[test]
fn discover_clean_repo() {
    let f = Fixture::new();
    let repo = f.repo();
    assert_eq!(repo.root(), f.root);
    let s = repo.status().unwrap();
    assert!(s.changes.is_empty());
    assert_eq!(s.branch.as_deref(), Some("main"));
}
#[test]
fn one_path_in_two_groups_has_different_diff() {
    let f = Fixture::new();
    f.write("a.txt", b"stage\ntwo\n");
    f.run(&["add", "a.txt"]);
    f.write("a.txt", b"working\ntwo\n");
    let r = f.repo();
    let staged = r.review(&f.entry(ChangeGroup::Staged)).unwrap();
    let unstaged = r.review(&f.entry(ChangeGroup::Unstaged)).unwrap();
    assert_ne!(staged.id, unstaged.id);
    assert!(staged.patch().windows(6).any(|x| x == b"+stage"));
    assert!(unstaged.patch().windows(8).any(|x| x == b"+working"));
}
#[test]
fn stage_and_unstage_touch_only_index() {
    let f = Fixture::new();
    f.write("a.txt", b"changed\n");
    let repo = f.repo();
    let review = repo.review(&f.entry(ChangeGroup::Unstaged)).unwrap();
    repo.apply_index(&review, IndexAction::Stage).unwrap();
    assert_eq!(std::fs::read(f.root.join("a.txt")).unwrap(), b"changed\n");
    let staged = repo.review(&f.entry(ChangeGroup::Staged)).unwrap();
    repo.apply_index(&staged, IndexAction::Unstage).unwrap();
    assert_eq!(f.entry(ChangeGroup::Unstaged).path.bytes(), b"a.txt");
    assert_eq!(std::fs::read(f.root.join("a.txt")).unwrap(), b"changed\n");
}
#[test]
fn stale_review_is_rejected() {
    let f = Fixture::new();
    f.write("a.txt", b"first\n");
    let repo = f.repo();
    let r = repo.review(&f.entry(ChangeGroup::Unstaged)).unwrap();
    f.write("a.txt", b"second\n");
    assert!(matches!(
        repo.apply_index(&r, IndexAction::Stage),
        Err(Error::Stale)
    ));
}

#[test]
fn retained_repository_rejects_same_path_checkout_replacement() {
    let f = Fixture::new();
    f.write("a.txt", b"changed\n");
    let repo = f.repo();
    let change = f.entry(ChangeGroup::Unstaged);
    let review = repo.review(&change).unwrap();
    let retained = f.parent.join("retained");
    std::fs::rename(&f.root, &retained).unwrap();
    std::fs::create_dir(&f.root).unwrap();
    f.run(&["init", "--initial-branch=main"]);
    f.write("a.txt", b"replacement\n");
    let replacement_index = f.run(&["ls-files", "--stage"]).stdout;
    assert!(repo.status().is_err());
    assert!(repo.review(&change).is_err());
    assert!(repo.apply_index(&review, IndexAction::Stage).is_err());
    assert_eq!(f.run(&["ls-files", "--stage"]).stdout, replacement_index);
    assert_eq!(
        std::fs::read(f.root.join("a.txt")).unwrap(),
        b"replacement\n"
    );
    assert_eq!(std::fs::read(retained.join("a.txt")).unwrap(), b"changed\n");
}

#[test]
fn retained_repository_rejects_replaced_admin_directory() {
    let f = Fixture::new();
    f.write("a.txt", b"changed\n");
    let repo = f.repo();
    let review = repo.review(&f.entry(ChangeGroup::Unstaged)).unwrap();
    let original_admin = f.parent.join("original-admin");
    std::fs::rename(f.root.join(".git"), original_admin).unwrap();
    f.run(&["init", "--initial-branch=main"]);
    let replacement_index = f.run(&["ls-files", "--stage"]).stdout;
    assert!(repo.validate_binding().is_err());
    assert!(repo.worktrees().is_err());
    assert!(repo.branches().is_err());
    assert!(repo.apply_index(&review, IndexAction::Stage).is_err());
    assert_eq!(f.run(&["ls-files", "--stage"]).stdout, replacement_index);
}

#[test]
fn retained_worktree_rejects_repointed_git_file_with_original_directories_alive() {
    let f = Fixture::new();
    let first = f.parent.join("first");
    let second = f.parent.join("second");
    f.run(&["worktree", "add", "-b", "first", first.to_str().unwrap()]);
    f.run(&["worktree", "add", "-b", "second", second.to_str().unwrap()]);
    std::fs::write(first.join("a.txt"), b"first\n").unwrap();
    let repo = Repository::discover(&f.git, &first).unwrap();
    let change = repo.status().unwrap().changes.remove(0);
    let review = repo.review(&change).unwrap();
    let first_index = std::fs::read(f.root.join(".git/worktrees/first/index")).unwrap();
    let second_index = std::fs::read(f.root.join(".git/worktrees/second/index")).unwrap();
    std::fs::write(
        first.join(".git"),
        std::fs::read(second.join(".git")).unwrap(),
    )
    .unwrap();
    assert!(repo.validate_binding().is_err());
    assert!(repo.status().is_err());
    assert!(repo.apply_index(&review, IndexAction::Stage).is_err());
    assert_eq!(
        std::fs::read(f.root.join(".git/worktrees/first/index")).unwrap(),
        first_index
    );
    assert_eq!(
        std::fs::read(f.root.join(".git/worktrees/second/index")).unwrap(),
        second_index
    );
}
#[test]
fn untracked_snapshot_is_staged_as_reviewed() {
    let f = Fixture::new();
    f.write("new.txt", b"new\n");
    let repo = f.repo();
    let r = repo.review(&f.entry(ChangeGroup::Untracked)).unwrap();
    repo.apply_index(&r, IndexAction::Stage).unwrap();
    assert_eq!(f.entry(ChangeGroup::Staged).path.bytes(), b"new.txt");
}
#[test]
fn rename_keeps_both_exact_paths() {
    let f = Fixture::new();
    f.run(&["mv", "a.txt", "new name.txt"]);
    let r = f.repo();
    let c = f.entry(ChangeGroup::Staged);
    assert_eq!(c.old_path.as_ref().unwrap().bytes(), b"a.txt");
    assert_eq!(c.path.bytes(), b"new name.txt");
    let patch = r.review(&c).unwrap();
    r.apply_index(&patch, IndexAction::Unstage).unwrap();
    assert!(f.root.join("new name.txt").exists());
}
#[test]
fn binary_patch_is_labelled_and_roundtrips() {
    let f = Fixture::new();
    f.write("binary", b"a\0b\x01");
    let r = f.repo();
    let v = r.review(&f.entry(ChangeGroup::Untracked)).unwrap();
    assert!(v.binary);
    r.apply_index(&v, IndexAction::Stage).unwrap();
    let v = r.review(&f.entry(ChangeGroup::Staged)).unwrap();
    r.apply_index(&v, IndexAction::Unstage).unwrap();
}
#[test]
fn odd_paths_are_literal_not_pathspecs() {
    let f = Fixture::new();
    for name in ["--output=hijack", ":(glob)*", "space and\nnewline.txt"] {
        f.write(name, b"data\n");
    }
    let r = f.repo();
    for c in r.status().unwrap().changes {
        let review = r.review(&c).unwrap();
        assert!(!review.patch().is_empty());
    }
    assert!(!f.root.join("hijack").exists());
}
#[test]
fn non_utf8_path_bytes_are_preserved() {
    let f = Fixture::new();
    let name = OsString::from_vec(b"bad-\xff.txt".to_vec());
    std::fs::write(f.root.join(name), b"text\n").unwrap();
    let c = f.entry(ChangeGroup::Untracked);
    assert_eq!(c.path.bytes(), b"bad-\xff.txt");
    assert!(c.path.label().contains("\\xff"));
    assert!(!f.repo().review(&c).unwrap().patch().is_empty());
}
#[test]
fn selecting_worktree_does_not_checkout_original() {
    let f = Fixture::new();
    let other = f.parent.join("other");
    f.run(&["worktree", "add", "-b", "feature", other.to_str().unwrap()]);
    let r = f.repo();
    let old = f.run(&["rev-parse", "HEAD"]).stdout;
    let new = r.select_worktree(&other).unwrap();
    assert_eq!(new.root(), other);
    assert_eq!(new.common_dir(), r.common_dir());
    assert_eq!(f.run(&["rev-parse", "HEAD"]).stdout, old);
    assert!(r.select_worktree(&f.parent).is_err());
}
#[test]
fn branch_open_is_compare_only() {
    let f = Fixture::new();
    f.run(&["branch", "other"]);
    let r = f.repo();
    let branch = r
        .branches()
        .unwrap()
        .into_iter()
        .find(|b| b.reference == "refs/heads/other")
        .unwrap();
    let v = r.compare(&branch).unwrap();
    assert!(!v.can_apply(IndexAction::Stage));
    assert_eq!(r.status().unwrap().branch.as_deref(), Some("main"));
}
#[test]
fn configured_clean_filter_is_never_executed() {
    let f = Fixture::new();
    f.run(&["config", "filter.bad.clean", "touch SHOULD_NOT_EXIST"]);
    f.write(".gitattributes", b"*.txt filter=bad\n");
    f.write("a.txt", b"updated\n");
    let r = f.repo();
    assert!(matches!(r.status(), Err(Error::Unsupported(_))));
    assert!(!f.root.join("SHOULD_NOT_EXIST").exists());
}
#[test]
fn external_diff_hook_is_disabled() {
    let f = Fixture::new();
    f.run(&["config", "diff.external", "touch SHOULD_NOT_EXIST"]);
    f.write("a.txt", b"updated\n");
    assert!(!f
        .repo()
        .review(&f.entry(ChangeGroup::Unstaged))
        .unwrap()
        .patch()
        .is_empty());
    assert!(!f.root.join("SHOULD_NOT_EXIST").exists());
}
#[test]
fn fsmonitor_hook_is_disabled() {
    let f = Fixture::new();
    f.run(&["config", "core.fsmonitor", "touch SHOULD_NOT_EXIST"]);
    f.repo().status().unwrap();
    assert!(!f.root.join("SHOULD_NOT_EXIST").exists());
}
#[test]
fn symlink_untracked_is_not_followed() {
    let f = Fixture::new();
    std::os::unix::fs::symlink("/etc/passwd", f.root.join("link")).unwrap();
    assert!(matches!(
        f.repo().review(&f.entry(ChangeGroup::Untracked)),
        Err(Error::Unsupported(_))
    ));
}
#[test]
fn unborn_repo_staging_works() {
    let f = Fixture::new();
    f.run(&["checkout", "--orphan", "unborn"]);
    f.run(&["rm", "--cached", "a.txt"]);
    let r = f.repo();
    let c = f.entry(ChangeGroup::Untracked);
    let review = r.review(&c).unwrap();
    r.apply_index(&review, IndexAction::Stage).unwrap();
}
#[test]
fn review_comments_bind_snapshot_and_line() {
    let f = Fixture::new();
    f.write("a.txt", b"updated\n");
    let r = f.repo().review(&f.entry(ChangeGroup::Unstaged)).unwrap();
    let i = r
        .lines
        .iter()
        .position(|l| l.kind == LineKind::Added)
        .unwrap();
    let c = r.comment(i, "Test this edge case").unwrap();
    assert_eq!(c.review_id, r.id);
    assert_eq!(c.new_line, Some(1));
    assert!(r.comment(usize::MAX, "text").is_err());
}
#[test]
fn outside_or_dotdot_paths_are_refused() {
    for p in [
        b"../a".as_slice(),
        b"/a",
        b".git/config",
        b"a/../b",
        b"",
        b"a\0b",
    ] {
        assert!(RepoPath::new(p.into()).is_err());
    }
}
#[test]
fn malformed_wire_is_refused() {
    for p in [
        b"1 malformed\0".as_slice(),
        b"? missing-final-null",
        b"wat\0",
    ] {
        assert!(parse_status(p).is_err());
    }
    assert!(parse_worktrees(b"branch refs/heads/main\0\0").is_err());
    assert!(parse_branches(b"not a branch").is_err());
}
#[test]
fn parser_handles_old_new_line_numbers() {
    let (bytes, limited) = parse_diff_lines(b"@@ -4,2 +6,2 @@\n old\n-gone\n+new\n", 20);
    assert!(!limited);
    assert_eq!(bytes[1].old, Some(4));
    assert_eq!(bytes[2].new, None);
    assert_eq!(bytes[3].new, Some(7));
}
#[test]
fn large_patch_refused_without_clipping_staging() {
    let f = Fixture::new();
    f.write("large", &vec![b'x'; MAX_PATCH_BYTES + 1]);
    assert!(f.repo().review(&f.entry(ChangeGroup::Untracked)).is_err());
}

#[test]
fn untracked_attribute_conversion_requires_external_git() {
    let f = Fixture::new();
    f.write(".gitattributes", b"new.txt text eol=lf\n");
    f.write("new.txt", b"raw\r\n");
    let repo = f.repo();
    let entry = repo
        .status()
        .unwrap()
        .changes
        .into_iter()
        .find(|c| c.path.bytes() == b"new.txt")
        .unwrap();
    assert!(matches!(repo.review(&entry), Err(Error::Unsupported(_))));
}
#[test]
fn undefined_filter_attribute_still_refused() {
    let f = Fixture::new();
    f.write(".gitattributes", b"a.txt filter=global-only\n");
    f.write("a.txt", b"changed\n");
    assert!(matches!(
        f.repo().review(&f.entry(ChangeGroup::Unstaged)),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn shared_raw_wire_vectors() {
    let v: serde_json::Value = serde_json::from_str(include_str!("../fixtures/wire.json")).unwrap();
    for case in v["status"].as_array().unwrap() {
        let bytes = case["wire"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as u8)
            .collect::<Vec<_>>();
        assert_eq!(
            parse_status(&bytes).unwrap().changes.len(),
            case["entries"].as_u64().unwrap() as usize
        );
    }
    for case in v["worktrees"].as_array().unwrap() {
        let bytes = case["wire"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as u8)
            .collect::<Vec<_>>();
        assert_eq!(
            parse_worktrees(&bytes).unwrap().len(),
            case["entries"].as_u64().unwrap() as usize
        );
    }
    for case in v["branches"].as_array().unwrap() {
        let bytes = case["wire"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as u8)
            .collect::<Vec<_>>();
        assert_eq!(
            parse_branches(&bytes).unwrap().len(),
            case["entries"].as_u64().unwrap() as usize
        );
    }
}

#[test]
fn managed_worktree_is_exact_detached_head_and_cleanup_is_explicit() {
    let f = Fixture::new();
    let repo = f.repo();
    let base = repo.clean_head_for_managed_worktree().unwrap();
    let managed_parent = f.parent.join("managed");
    std::fs::create_dir(&managed_parent).unwrap();
    let target = managed_parent.join("child");

    let worktree = repo.create_managed_worktree(&target, &base).unwrap();
    assert_eq!(worktree.head.as_deref(), Some(base.as_str()));
    assert!(worktree.detached);
    assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), b"one\ntwo\n");
    let child = Repository::discover(&f.git, &target).unwrap();
    assert_eq!(child.common_dir(), repo.common_dir());
    assert!(child.status().unwrap().changes.is_empty());

    std::fs::write(target.join("a.txt"), b"child change\n").unwrap();
    assert!(matches!(
        repo.remove_managed_worktree(&target, false),
        Err(Error::Stale)
    ));
    assert!(target.exists());
    repo.remove_managed_worktree(&target, true).unwrap();
    assert!(!target.exists());
    assert!(!repo
        .worktrees()
        .unwrap()
        .iter()
        .any(|item| item.path == target));
}

#[test]
fn managed_worktree_refuses_dirty_parent_before_git_mutation() {
    let f = Fixture::new();
    f.write("a.txt", b"dirty\n");
    let repo = f.repo();
    assert!(matches!(
        repo.clean_head_for_managed_worktree(),
        Err(Error::Unsupported(_))
    ));
    let target = f.parent.join("should-not-exist");
    assert!(!target.exists());
}
