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
