//! Behavioral compatibility tests for the local dotlink-aligned tool surface.
//! No running model, credentials or MCP transport is needed.
use super::*;
use aw_agent_core::{
    AccountOwner, BillingOwner, CancellationToken, CredentialCustody, InvocationIdentity, Tool,
    ToolContext, ToolOutput,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "aw-tool-port-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
    fn root(&self) -> Arc<aw_platform::scoped_fs::Root> {
        Arc::new(aw_platform::scoped_fs::Root::open(&self.0).unwrap())
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn context() -> ToolContext {
    let actor = PrincipalId::new("local").unwrap();
    ToolContext {
        actor: actor.clone(),
        turn_id: 1,
        execution_site: "local".into(),
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
async fn call<T: Tool>(tool: &T, arguments: Value) -> ToolOutput {
    tool.execute(context(), &arguments)
        .await
        .expect("tool execution")
}
#[tokio::test]
async fn text_window_and_atomic_replacement_preserve_raw_bytes() {
    let dir = Tree::new();
    fs::write(dir.path("read.txt"), "alpha\r\nbeta\r\n").unwrap();
    let root = dir.root();
    let read = text::Text::new(root.clone(), text::Operation::Read);
    let exact = call(
        &read,
        json!({"path":"read.txt","offset":2,"limit":1,"numbered":false}),
    )
    .await;
    assert_eq!(exact.content, "beta\r\n");
    let numbered = call(&read, json!({"path":"read.txt","offset":2,"limit":1})).await;
    assert_eq!(numbered.content, "2\tbeta\n");
    let writer = text::Text::new(root, text::Operation::Write);
    let receipt = call(&writer, json!({"path":"read.txt","content":"replaced\n"})).await;
    assert!(!receipt.is_error, "{}", receipt.content);
    assert_eq!(
        fs::read_to_string(dir.path("read.txt")).unwrap(),
        "replaced\n"
    );
    let created = call(
        &writer,
        json!({"path":"nested/deep/new.txt","content":"brand new"}),
    )
    .await;
    assert!(!created.is_error, "{}", created.content);
    assert_eq!(
        fs::read_to_string(dir.path("nested/deep/new.txt")).unwrap(),
        "brand new"
    );
    let leaked = fs::read_dir(&dir.0)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with(".aw-stage-"));
    assert!(!leaked);
}
#[tokio::test]
async fn find_and_grep_have_orthogonal_discovery_and_content_modes() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("src/deep")).unwrap();
    fs::write(dir.path("src/deep/code.rs"), "TEST\nTEST\n").unwrap();
    fs::write(dir.path("src/deep/other.txt"), "TEST\n").unwrap();
    fs::write(dir.path(".hidden"), "TEST\n").unwrap();
    let root = dir.root();
    let find = search::Search::new(root.clone(), search::Operation::Find);
    let result = call(&find, json!({"glob":"**/*.rs","kind":"file"})).await;
    assert!(!result.is_error);
    assert!(
        result.content.lines().any(|s| s == "f  src/deep/code.rs"),
        "{}",
        result.content
    );
    let grep = search::Search::new(root, search::Operation::Grep);
    let counted = call(
        &grep,
        json!({"pattern":"TEST","glob":"**/*.rs","mode":"count"}),
    )
    .await;
    assert_eq!(counted.content, "2  src/deep/code.rs\n");
    let filenames = call(
        &grep,
        json!({"pattern":"TEST","glob":"**/*.rs","mode":"files"}),
    )
    .await;
    assert_eq!(filenames.content, "src/deep/code.rs\n");
    let text = call(&grep, json!({"pattern":"TEST","path":"src/deep","limit":1})).await;
    assert!(text.content.contains("src/deep/code.rs:1:TEST"));
    assert!(text.truncated);
}
#[tokio::test]
async fn exact_multi_edit_and_codex_patch_share_atomic_backend() {
    let dir = Tree::new();
    fs::write(dir.path("a.txt"), "one\ntwo\n").unwrap();
    fs::write(dir.path("b.txt"), "three\n").unwrap();
    let root = dir.root();
    let edit = edit::Edit::new(root.clone());
    let receipt = call(
        &edit,
        json!({"edits":[
        {"path":"a.txt","old_text":"one","new_text":"ONE"},
        {"path":"a.txt","old_text":"two","new_text":"TWO"},
        {"path":"b.txt","old_text":"three","new_text":"THREE"}]}),
    )
    .await;
    assert!(!receipt.is_error, "{}", receipt.content);
    assert_eq!(fs::read_to_string(dir.path("a.txt")).unwrap(), "ONE\nTWO\n");
    assert_eq!(fs::read_to_string(dir.path("b.txt")).unwrap(), "THREE\n");
    let bad = call(
        &edit,
        json!({"edits":[
        {"path":"a.txt","old_text":"ONE","new_text":"oops"},
        {"path":"b.txt","old_text":"absent","new_text":"oops"}]}),
    )
    .await;
    assert!(bad.is_error);
    assert_eq!(fs::read_to_string(dir.path("a.txt")).unwrap(), "ONE\nTWO\n");
    let patch = patch::Patch::new(root);
    let receipt=call(&patch,json!({"patch":
        "*** Begin Patch\n*** Update File: a.txt\n@@\n-ONE\n+new first line\n TWO\n*** Add File: new.txt\n+hello\n*** End Patch\n"})).await;
    assert!(!receipt.is_error, "{}", receipt.content);
    assert_eq!(
        fs::read_to_string(dir.path("a.txt")).unwrap(),
        "new first line\nTWO\n"
    );
    assert_eq!(fs::read_to_string(dir.path("new.txt")).unwrap(), "hello\n");
}
#[tokio::test]
async fn binary_window_and_multi_range_edits_have_preimage_checks() {
    let dir = Tree::new();
    let root = dir.root();
    let write = binary::Binary::new(root.clone(), binary::Operation::Write);
    let receipt = call(&write, json!({"path":"blob.bin","data":"AAECAwQ="})).await;
    assert!(!receipt.is_error, "{}", receipt.content);
    let read = binary::Binary::new(root.clone(), binary::Operation::Read);
    let result = call(
        &read,
        json!({"path":"blob.bin","offset":1,"limit":2,"format":"hex"}),
    )
    .await;
    let data: Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(data["data"], "0102");
    assert_eq!(data["total_bytes"], 5);
    assert_eq!(data["truncated"], true);
    let edit = binary::Binary::new(root, binary::Operation::Edit);
    let bad=call(&edit,json!({"path":"blob.bin","sha256":"0000000000000000000000000000000000000000000000000000000000000000",
        "edits":[{"offset":2,"length":1,"data":"Wg=="}]})).await;
    assert!(bad.is_error);
    assert_eq!(fs::read(dir.path("blob.bin")).unwrap(), [0, 1, 2, 3, 4]);
    let receipt = call(
        &edit,
        json!({"path":"blob.bin","edits":[
        {"offset":2,"length":1,"data":"Wg=="},
        {"offset":4,"length":0,"data":"WQ=="}]}),
    )
    .await;
    assert!(!receipt.is_error, "{}", receipt.content);
    assert_eq!(
        fs::read(dir.path("blob.bin")).unwrap(),
        [0, 1, b'Z', 3, b'Y', 4]
    );
}
#[tokio::test]
async fn read_does_not_wait_for_a_mutation_lock() {
    let dir = Tree::new();
    fs::write(dir.path("same.txt"), "safe\n").unwrap();
    let root = dir.root();
    let guard = root.lock_mutations(&["same.txt".to_owned()]).unwrap();
    // A read must not accidentally take the writer's root mutex.
    let (body, more) = root.read_window("same.txt", 1, 5, 2048, false).unwrap();
    assert_eq!(body, "safe\n");
    assert!(!more);
    drop(guard);
}

#[tokio::test]
async fn scoped_globs_are_relative_to_requested_directory() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("src/nested/deep")).unwrap();
    fs::write(dir.path("src/nested/deep/file.rs"), "HELLO\n").unwrap();
    fs::write(dir.path("src/nested/deep/other.py"), "HELLO\n").unwrap();
    let root = dir.root();
    let ls = search::Search::new(root.clone(), search::Operation::Find);
    let listed = call(
        &ls,
        json!({"path":"src","glob":"nested/**/*.rs","kind":"file"}),
    )
    .await;
    assert_eq!(listed.content, "f  src/nested/deep/file.rs\n");

    let grep = search::Search::new(root, search::Operation::Grep);
    let matches = call(
        &grep,
        json!({"path":"src","glob":"nested/**/*.rs","pattern":"HELLO","mode":"files"}),
    )
    .await;
    assert_eq!(matches.content, "src/nested/deep/file.rs\n");
}
#[tokio::test]
async fn duplicate_patch_and_failed_preflight_publish_nothing() {
    let dir = Tree::new();
    fs::write(dir.path("other.txt"), "unchanged\n").unwrap();
    let patch = patch::Patch::new(dir.root());
    let duplicate=call(&patch,json!({"patch":
        "*** Begin Patch\n*** Add File: z.txt\n+first\n*** Add File: z.txt\n+second\n*** End Patch\n"})).await;
    assert!(duplicate.is_error);
    assert!(!dir.path("z.txt").exists());
    let missing=call(&patch,json!({"patch":
        "*** Begin Patch\n*** Add File: z.txt\n+first\n*** Update File: other.txt\n@@\n-never was here\n+changed\n*** End Patch\n"})).await;
    assert!(missing.is_error);
    assert!(!dir.path("z.txt").exists());
    assert_eq!(
        fs::read_to_string(dir.path("other.txt")).unwrap(),
        "unchanged\n"
    );
}
#[tokio::test]
async fn best_effort_receipts_exclude_failed_sections_and_preserve_crlf() {
    let dir = Tree::new();
    fs::write(dir.path("crlf.txt"), "old\r\nkeep\r\n").unwrap();
    let patch = patch::Patch::new(dir.root());
    let result=call(&patch,json!({"best_effort":true,"patch":
        "*** Begin Patch\n*** Update File: crlf.txt\n@@\n-old\n+new\n keep\n*** Update File: absent.txt\n@@\n-nothing\n+something\n*** Add File: new.txt\n+hello\n*** End Patch\n"})).await;
    assert!(result.is_error);
    assert!(
        result.content.starts_with("mixed ok=2 fail=1"),
        "{}",
        result.content
    );
    assert_eq!(
        fs::read_to_string(dir.path("crlf.txt")).unwrap(),
        "new\r\nkeep\r\n"
    );
    assert_eq!(fs::read_to_string(dir.path("new.txt")).unwrap(), "hello\n");
}
#[tokio::test]
async fn canonical_read_pages_unicode_inside_long_lines() {
    let tree = Tree::new();
    let source = format!("{}\nlast\n", "🙂é".repeat(20_000));
    fs::write(tree.path("long.txt"), &source).unwrap();
    let read = text::Text::new(tree.root(), text::Operation::Read);
    let mut cursor = aw_tools_core::read::Cursor { line: 1, byte: 0 };
    let mut output = String::new();
    for attempt in 0..50 {
        let response = call(
            &read,
            serde_json::json!({
                "path":"long.txt","line":cursor.line,
                "byte":cursor.byte,"lines":100,"numbered":false
            }),
        )
        .await;
        assert!(!response.is_error, "{}", response.content);
        if response.truncated {
            let json: Value = serde_json::from_str(&response.content).unwrap();
            output.push_str(json["data"].as_str().unwrap());
            assert_eq!(json["status"], "more");
            let next = aw_tools_core::read::Cursor {
                line: json["next"]["line"].as_u64().unwrap() as usize,
                byte: json["next"]["byte"].as_u64().unwrap() as usize,
            };
            assert!(
                next.line > cursor.line || next.byte > cursor.byte,
                "no progress at attempt {attempt}"
            );
            cursor = next;
        } else {
            output.push_str(&response.content);
            assert_eq!(output, source);
            return;
        }
    }
    panic!("read never terminated");
}
#[tokio::test]
async fn canonical_read_cursor_is_continuous_between_lines() {
    let tree = Tree::new();
    fs::write(tree.path("three.txt"), "α\nβ\nγ\n").unwrap();
    let read = text::Text::new(tree.root(), text::Operation::Read);
    let first = call(&read, json!({"path":"three.txt","lines":2})).await;
    assert!(first.truncated);
    assert!(
        first.content.ends_with("more line=3 byte=0\n"),
        "one canonical status line: {:?}",
        first.content
    );
    assert!(
        !first.content.ends_with("\n\n"),
        "read status must not append an empty line"
    );
    let next = call(&read, json!({"path":"three.txt","line":3,"lines":10})).await;
    assert!(!next.truncated);
    assert_eq!(next.content, "3\tγ\n");
    let invalid = call(
        &read,
        json!({"path":"three.txt","line":1,"byte":1,
        "numbered":false}),
    )
    .await;
    assert!(invalid.is_error);
}
#[tokio::test]
async fn line_window_skips_large_preceding_line_in_constant_space() {
    let dir = Tree::new();
    let mut bytes = vec![b'x'; 1024 * 1024 + 33];
    bytes.extend_from_slice(b"\nselected\r\n");
    fs::write(dir.path("giant.txt"), bytes).unwrap();
    let root = dir.root();
    let read = text::Text::new(root, text::Operation::Read);
    let result = call(
        &read,
        json!({"path":"giant.txt","offset":2,"limit":1,"numbered":false}),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(result.content, "selected\r\n");
}
#[cfg(unix)]
#[tokio::test]
async fn scoped_writes_keep_existing_mode_and_refuse_symlinks() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = Tree::new();
    fs::write(dir.path("private.sh"), "old").unwrap();
    fs::set_permissions(dir.path("private.sh"), fs::Permissions::from_mode(0o700)).unwrap();
    let root = dir.root();
    let guard = root.lock_mutations(&["private.sh".to_owned()]).unwrap();
    let staged = root.stage_replace("private.sh", b"new", &|| false).unwrap();
    let pending = fs::read_dir(&dir.0)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".aw-stage-")
        })
        .expect("private staged sibling");
    let stage_mode = fs::metadata(&pending).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        stage_mode, 0o700,
        "staging never broadens existing permissions"
    );
    assert_eq!(fs::read_to_string(dir.path("private.sh")).unwrap(), "old");
    staged.commit().unwrap();
    drop(guard);
    assert_eq!(
        fs::metadata(dir.path("private.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    symlink(dir.path("private.sh"), dir.path("link.txt")).unwrap();
    let writer = text::Text::new(root, text::Operation::Write);
    let result = call(&writer, json!({"path":"link.txt","content":"unsafe"})).await;
    assert!(result.is_error);
    assert_eq!(fs::read_to_string(dir.path("private.sh")).unwrap(), "new");
}
#[tokio::test]
async fn staging_stale_snapshot_and_cancel_leave_old_bytes() {
    let dir = Tree::new();
    fs::write(dir.path("safe.txt"), "before").unwrap();
    let root = dir.root();
    let _guard = root.lock_mutations(&["safe.txt".to_owned()]).unwrap();
    let staged = root
        .stage_update("safe.txt", b"before", b"after", &|| false)
        .unwrap();
    fs::write(dir.path("safe.txt"), "external").unwrap();
    assert!(staged.commit().is_err());
    assert_eq!(
        fs::read_to_string(dir.path("safe.txt")).unwrap(),
        "external"
    );
    let cancelled = root.stage_replace("safe.txt", b"new", &|| true);
    assert!(cancelled.is_err());
    assert_eq!(
        fs::read_to_string(dir.path("safe.txt")).unwrap(),
        "external"
    );
    assert!(!fs::read_dir(&dir.0)
        .unwrap()
        .flatten()
        .any(|p| p.file_name().to_string_lossy().starts_with(".aw-stage-")));
}
#[test]
fn concurrent_root_listing_keeps_independent_cursors() {
    let dir = Tree::new();
    for n in 0..200 {
        fs::write(dir.path(&format!("file{n:03}.txt")), b"x").unwrap();
    }
    let root = dir.root();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let root = root.clone();
            scope.spawn(move || {
                for _ in 0..15 {
                    let listing = root.list_with_limit("", 512).unwrap();
                    assert_eq!(listing.entries.len(), 200, "root dir cursor raced");
                    assert!(!listing.incomplete);
                }
            });
        }
    });
}
#[test]
fn cooperative_cross_process_writer_helper() {
    let Ok(path) = std::env::var("AW_TOOL_MUTATION_CHILD_ROOT") else {
        return;
    };
    let root = aw_platform::scoped_fs::Root::open(&PathBuf::from(path)).unwrap();
    let _guard = root.lock_mutations(&["shared.txt".to_string()]).unwrap();
    let original = root.read_bytes("shared.txt", 16384).unwrap();
    let mut after = original.clone();
    after.push(b'X');
    root.stage_update("shared.txt", &original, &after, &|| false)
        .unwrap()
        .commit()
        .unwrap();
}
#[test]
fn independent_aw_processes_do_not_lose_successful_mutations() {
    let dir = Tree::new();
    fs::write(dir.path("shared.txt"), "start").unwrap();
    let executable = std::env::current_exe().unwrap();
    let mut children = (0..6)
        .map(|_| {
            std::process::Command::new(&executable)
                .arg("--exact")
                .arg("tool_tests::cooperative_cross_process_writer_helper")
                .env("AW_TOOL_MUTATION_CHILD_ROOT", &dir.0)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn independent AW tool process")
        })
        .collect::<Vec<_>>();
    for child in children.drain(..) {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let body = fs::read(dir.path("shared.txt")).unwrap();
    assert_eq!(
        body.iter().filter(|&&b| b == b'X').count(),
        6,
        "every acknowledged insert survives"
    );
}

#[tokio::test]
async fn find_global_topn_sort_and_unreadable_candidate() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("a")).unwrap();
    fs::write(dir.path("a/first.rs"), "FIRST").unwrap();
    fs::write(dir.path("z.rs"), "SECOND").unwrap();
    let ls = search::Search::new(dir.root(), search::Operation::Find);
    let res = call(&ls, json!({"glob":"**/*.rs","limit":1,"sort":"path"})).await;
    assert!(
        res.content.starts_with("f  a/first.rs\n"),
        "{}",
        res.content
    );
    assert!(res.truncated);
    assert!(res.content.contains("more offset="));
    // Preserve precise nanosecond mtime sort, even for same-second writes.
    let newer = dir.path("z.rs");
    std::thread::sleep(std::time::Duration::from_millis(15));
    fs::write(&newer, "NEWEST").unwrap();
    let res = call(&ls, json!({"glob":"**/*.rs","limit":1,"sort":"mtime"})).await;
    assert!(res.content.starts_with("f  z.rs\n"), "{}", res.content);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let a = dir.path("a/first.rs");
        fs::set_permissions(&a, fs::Permissions::from_mode(0o0)).unwrap();
        let res = call(&ls, json!({"glob":"**/*.rs","limit":2,"sort":"path"})).await;
        assert_eq!(
            res.content
                .lines()
                .filter(|line| line.starts_with("f  "))
                .count(),
            2
        );
        fs::set_permissions(&a, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
#[tokio::test]
async fn find_can_ignore_git_and_hidden_files() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("target")).unwrap();
    fs::create_dir_all(dir.path("src")).unwrap();
    fs::write(dir.path(".gitignore"), "target/\n").unwrap();
    fs::write(dir.path("target/generated.rs"), "ignored\n").unwrap();
    fs::write(dir.path("src/lib.rs"), "useful\n").unwrap();
    fs::write(dir.path(".hidden.rs"), "hidden\n").unwrap();
    let ls = search::Search::new(dir.root(), search::Operation::Find);
    let result = call(
        &ls,
        json!({"glob":"**/*.rs","gitignore":true,
        "kind":"file"}),
    )
    .await;
    assert_eq!(result.content, "f  src/lib.rs\n");
}
#[tokio::test]
async fn grep_filters_exact_file_and_classifies_binary_consistently() {
    let dir = Tree::new();
    fs::write(dir.path("code.rs"), "NEEDLE\n").unwrap();
    fs::write(dir.path("binary.txt"), b"NEEDLE\0ALSO\x1b[31m\n").unwrap();
    fs::write(dir.path(".gitignore"), "code.rs\n").unwrap();
    let root = dir.root();
    let grep = search::Search::new(root.clone(), search::Operation::Grep);
    let wrong_glob = call(
        &grep,
        json!({"path":"code.rs","pattern":"NEEDLE",
        "glob":"*.py","mode":"files"}),
    )
    .await;
    assert_eq!(wrong_glob.content, "");
    let ignored = call(
        &grep,
        json!({"path":"code.rs","pattern":"NEEDLE",
        "gitignore":true,"mode":"files"}),
    )
    .await;
    assert_eq!(ignored.content, "");
    let all = call(&grep, json!({"pattern":"NEEDLE","mode":"files"})).await;
    assert_eq!(all.content, "code.rs\n");
    let reader = text::Text::new(root, text::Operation::Read);
    let invalid = call(&reader, json!({"path":"binary.txt"})).await;
    assert!(invalid.is_error);
}
#[cfg(unix)]
#[tokio::test]
async fn canonical_write_does_not_require_read_permission_on_existing_file() {
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::new();
    fs::write(tree.path("write_only.txt"), b"secret").unwrap();
    fs::set_permissions(
        tree.path("write_only.txt"),
        fs::Permissions::from_mode(0o200),
    )
    .unwrap();
    let write = text::Text::new(tree.root(), text::Operation::Write);
    let result = write
        .execute(
            context(),
            &json!({"path":"write_only.txt",
        "content":"published"}),
        )
        .await;
    match result {
        Ok(output) => assert!(!output.is_error, "{}", output.content),
        Err(error) => panic!("write-only target failed: {error:?}"),
    }
    let mode = fs::metadata(tree.path("write_only.txt"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o200);
    fs::set_permissions(
        tree.path("write_only.txt"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert_eq!(fs::read(tree.path("write_only.txt")).unwrap(), b"published");
}
#[tokio::test]
async fn conditional_text_binary_writes_and_idempotent_receipts() {
    use sha2::Digest;
    let dir = Tree::new();
    let root = dir.root();
    let text = text::Text::new(root.clone(), text::Operation::Write);
    let first = call(
        &text,
        json!({"path":"file.txt","content":"same","create_only":true}),
    )
    .await;
    assert!(!first.is_error);
    #[cfg(unix)]
    let before = fs::metadata(dir.path("file.txt")).unwrap();
    let again = call(&text, json!({"path":"file.txt","content":"same"})).await;
    assert_eq!(again.content, "ok");
    #[cfg(unix)]
    let after = fs::metadata(dir.path("file.txt")).unwrap();
    assert_eq!(fs::read(dir.path("file.txt")).unwrap(), b"same");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_ne!(
            before.ino(),
            after.ino(),
            "write-only canonical publication must not be an equality oracle"
        );
    }
    let refused = call(
        &text,
        json!({"path":"file.txt","content":"other",
        "create_only":true}),
    )
    .await;
    assert!(refused.is_error);
    let sha = format!("{:x}", sha2::Sha256::digest(b"same"));
    let wrong = call(
        &text,
        json!({"path":"file.txt","content":"bad",
        "expected_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}),
    )
    .await;
    assert!(wrong.is_error);
    let expected = call(
        &text,
        json!({"path":"file.txt","content":"updated",
        "expected_sha256":sha}),
    )
    .await;
    assert!(!expected.is_error, "{}", expected.content);
    assert_eq!(fs::read_to_string(dir.path("file.txt")).unwrap(), "updated");
    let binary = binary::Binary::new(root, binary::Operation::Write);
    let first = call(&binary, json!({"path":"file.bin","data":"AQID"})).await;
    assert!(!first.is_error);
    let noop = call(&binary, json!({"path":"file.bin","data":"AQID"})).await;
    assert_eq!(noop.content, "ok");
}
#[tokio::test]
async fn mcp_format_accepts_complete_file_only_and_preserves_mime() {
    let dir = Tree::new();
    fs::write(dir.path("small.png"), b"\x89PNG\r\n\x1a\nbinary-payload").unwrap();
    let read = binary::Binary::new(dir.root(), binary::Operation::Read);
    let result = call(&read, json!({"path":"small.png","format":"mcp"})).await;
    assert!(!result.is_error);
    let value: Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(value["type"], "image");
    assert_eq!(value["mime_type"], "image/png");
    assert_eq!(value["mime_source"], "signature");
    assert_eq!(value["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["transport"], "encoded_json");
    assert!(read
        .validate(&json!({"path":"small.png","format":"mcp","offset":1}))
        .is_err());
    let small = call(&read, json!({"path":"small.png","format":"mcp","limit":2})).await;
    assert!(small.is_error);
}
#[tokio::test]
async fn encoded_media_never_trusts_filename_extension() {
    let tree = Tree::new();
    fs::write(tree.path("spoof.png"), b"not a valid PNG signature").unwrap();
    let read = binary::Binary::new(tree.root(), binary::Operation::Read);
    let out = call(&read, json!({"path":"spoof.png","format":"mcp"})).await;
    assert!(!out.is_error, "{}", out.content);
    let result: Value = serde_json::from_str(&out.content).unwrap();
    assert_eq!(result["type"], "resource");
    assert_eq!(result["mime_type"], "application/octet-stream");
    assert_eq!(result["mime_source"], "fallback");
    assert_eq!(result["transport"], "encoded_json");
}
#[tokio::test]
async fn strict_patch_failure_cleans_staged_files_and_empty_parent_directories() {
    let dir = Tree::new();
    fs::write(dir.path("exists.txt"), "old\n").unwrap();
    let patch = patch::Patch::new(dir.root());
    let failed=call(&patch,json!({"patch":
        "*** Begin Patch\n*** Add File: nested/deep/new.txt\n+hello\n*** Add File: exists.txt\n+overwrite\n*** End Patch\n"})).await;
    assert!(failed.is_error);
    assert!(!dir.path("nested/deep/new.txt").exists());
    assert!(
        !dir.path("nested").exists(),
        "aborted strict patch left an empty directory"
    );
    assert_eq!(fs::read_to_string(dir.path("exists.txt")).unwrap(), "old\n");
}
#[tokio::test]
async fn skipped_text_offset_has_finite_32mib_budget() {
    let dir = Tree::new();
    // The selected line is tiny, but an unrestricted seek would scan many GiB.
    let mut bytes = vec![b'X'; 32 * 1024 * 1024 + 64];
    bytes.extend_from_slice(b"\nfound\n");
    fs::write(dir.path("huge.txt"), bytes).unwrap();
    let reader = text::Text::new(dir.root(), text::Operation::Read);
    let result = call(&reader, json!({"path":"huge.txt","offset":2,"limit":1})).await;
    assert!(result.is_error);
    assert!(result.content.contains("32 MiB"), "{}", result.content);
}
#[tokio::test]
async fn grep_total_scan_budget_is_reported_and_not_silent() {
    let dir = Tree::new();
    // One directory of 33 MiB total: exceeding a call's 32 MiB scan budget.
    for n in 0..34 {
        fs::write(
            dir.path(&format!("file{n:03}.txt")),
            vec![b'Q'; 1024 * 1024 - 10],
        )
        .unwrap();
    }
    let grep = search::Search::new(dir.root(), search::Operation::Grep);
    let response = call(&grep, json!({"pattern":"missing","mode":"files"})).await;
    assert!(response.truncated);
    assert!(
        response.content.contains("incomplete budget"),
        "{}",
        response.content
    );
}
#[cfg(unix)]
#[tokio::test]
async fn shell_accepts_all_dotlink_inputs_without_widening_execution() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("child")).unwrap();
    let Some(executable) = std::env::var_os("AW_BASH").map(PathBuf::from) else {
        return;
    };
    let shell = shell::Bash::new(dir.0.clone(), executable).unwrap();
    let response = call(
        &shell,
        json!({"command":"read x; printf '%s' \"$x\"",
        "dir":"child","stdin":"greetings\n","timeout_secs":3,
        "max_output_bytes":100,"parallel":false}),
    )
    .await;
    assert!(!response.is_error, "{}", response.content);
    assert!(response.content.contains("greetings"));
    let escape = shell.validate(&json!({"command":"pwd","dir":"../escape"}));
    assert!(escape.is_err());
}
#[test]
fn tool_schemas_cover_dl150_ai_native_surface() {
    let tree = Tree::new();
    let root = tree.root();
    let surface: Vec<(ToolSpecVec, &[&str])> = vec![
        (
            ToolSpecVec::new(search::Search::new(root.clone(), search::Operation::Ls)),
            &[
                "path",
                "sort",
                "meta",
                "hidden",
                "gitignore",
                "offset",
                "limit",
                "parallel",
            ],
        ),
        (
            ToolSpecVec::new(search::Search::new(root.clone(), search::Operation::Find)),
            &[
                "path",
                "glob",
                "regex",
                "depth",
                "kind",
                "mtime",
                "ctime",
                "btime",
                "sort",
                "meta",
                "hidden",
                "gitignore",
                "offset",
                "limit",
                "parallel",
            ],
        ),
        (
            ToolSpecVec::new(search::Search::new(root.clone(), search::Operation::Grep)),
            &[
                "pattern",
                "path",
                "glob",
                "depth",
                "literal",
                "ignore_case",
                "mode",
                "context",
                "hidden",
                "gitignore",
                "offset",
                "limit",
                "parallel",
            ],
        ),
        (
            ToolSpecVec::new(text::Text::new(root.clone(), text::Operation::Read)),
            &["path", "line", "lines", "byte", "numbered", "parallel"],
        ),
        (
            ToolSpecVec::new(text::Text::new(root.clone(), text::Operation::Write)),
            &["path", "content"],
        ),
        (
            ToolSpecVec::new(edit::Edit::new(root.clone())),
            &["path", "edits"],
        ),
        (
            ToolSpecVec::new(patch::Patch::new(root.clone())),
            &["patch"],
        ),
        (
            ToolSpecVec::new(binary::Binary::new(root.clone(), binary::Operation::Read)),
            &["path", "offset", "limit", "format", "parallel"],
        ),
        (
            ToolSpecVec::new(binary::Binary::new(root.clone(), binary::Operation::Write)),
            &["path", "data", "encoding"],
        ),
        (
            ToolSpecVec::new(binary::Binary::new(root, binary::Operation::Edit)),
            &["path", "edits", "sha256"],
        ),
    ];
    for (tool, expected) in surface {
        for key in expected {
            assert!(
                tool.parameters["properties"].get(key).is_some(),
                "{} missing {key}",
                tool.name
            );
        }
        if tool.name == "edit" {
            let props = &tool.parameters["properties"];
            for key in ["path", "edits"] {
                assert!(props.get(key).is_some(), "edit schema missing {key}");
            }
            for legacy in ["old", "new", "all"] {
                assert!(props.get(legacy).is_none(), "edit schema exposes {legacy}");
            }
        }
        if tool.name == "ls" {
            assert!(tool.parameters["properties"].get("depth").is_none());
            assert!(tool.parameters["properties"].get("pattern").is_none());
        }
    }
}
struct ToolSpecVec {
    name: String,
    parameters: Value,
}
impl ToolSpecVec {
    fn new(tool: impl aw_agent_core::Tool) -> Self {
        let spec = tool.spec();
        Self {
            name: spec.name,
            parameters: spec.parameters,
        }
    }
}

#[tokio::test]
async fn gitignore_budgets_reject_oversized_files_and_cumulative_work() {
    let dir = Tree::new();
    fs::write(dir.path("matching.txt"), "hello\n").unwrap();
    fs::write(dir.path(".gitignore"), "matching.txt\n").unwrap();
    let root = dir.root();
    let grep = search::Search::new(root.clone(), search::Operation::Grep);
    let filtered = call(
        &grep,
        json!({"pattern":"hello","gitignore":true,"mode":"files"}),
    )
    .await;
    assert_eq!(filtered.content, "");
    fs::write(dir.path(".gitignore"), "x".repeat(64 * 1024 + 1)).unwrap();
    let oversized = call(
        &grep,
        json!({"pattern":"hello","gitignore":true,"mode":"files"}),
    )
    .await;
    assert!(!oversized.is_error, "{}", oversized.content);
    assert!(
        oversized.content.contains("incomplete gitignore"),
        "oversized ignore must preserve safe results and coverage warning"
    );
    // Hundreds of tiny ignore files must be covered by an aggregate
    // budget, independently of the content-matching scan byte ceiling.
    fs::write(dir.path(".gitignore"), "# okay\n").unwrap();
    for depth in 0..258 {
        let sub = format!("d{depth:03}");
        fs::create_dir(dir.path(&sub)).unwrap();
        fs::write(dir.path(&format!("{sub}/.gitignore")), "#ignored\n").unwrap();
    }
    let capped = call(
        &grep,
        json!({"pattern":"hello","gitignore":true,"mode":"files"}),
    )
    .await;
    assert!(!capped.is_error, "{}", capped.content);
    assert!(
        capped.content.contains("incomplete gitignore"),
        "ignore work exhaustion is incomplete, never a hard request failure"
    );
}
#[tokio::test]
async fn text_read_refuses_binary_skipped_prefix() {
    let dir = Tree::new();
    fs::write(dir.path("bad.txt"), b"BAD\0\nGOOD\n").unwrap();
    let reader = text::Text::new(dir.root(), text::Operation::Read);
    let result = call(&reader, json!({"path":"bad.txt","offset":2,"limit":1})).await;
    assert!(result.is_error, "{}", result.content);
}
#[test]
fn native_shell_schema_retains_all_arguments() {
    #[cfg(unix)]
    {
        let root = Tree::new();
        let tool = shell::Bash::new(root.0.clone(), PathBuf::from("/usr/bin/env")).unwrap();
        let spec = tool.spec();
        assert_eq!(spec.name, "bash");
        for key in [
            "command",
            "dir",
            "stdin",
            "timeout_secs",
            "max_output_bytes",
            "parallel",
        ] {
            assert!(spec.parameters["properties"].get(key).is_some(), "{key}");
        }
    }
}

#[tokio::test]
async fn patch_best_effort_skips_invalid_path_and_unchanged_update() {
    let dir = Tree::new();
    fs::write(dir.path("same.txt"), "same\n").unwrap();
    let patch = patch::Patch::new(dir.root());
    let output=call(&patch,json!({"best_effort":true,"patch":
        "*** Begin Patch\n*** Add File: ../escape.txt\n+wrong\n*** Update File: same.txt\n@@\n-same\n+same\n*** Add File: good.txt\n+right\n*** End Patch\n"})).await;
    assert!(output.is_error, "{}", output.content);
    assert!(
        output.content.starts_with("mixed ok=1 fail=1"),
        "{}",
        output.content
    );
    assert_eq!(fs::read_to_string(dir.path("same.txt")).unwrap(), "same\n");
    assert_eq!(fs::read_to_string(dir.path("good.txt")).unwrap(), "right\n");
}

#[cfg(unix)]
#[tokio::test]
async fn unreadable_subtrees_and_files_are_reported_as_incomplete() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Tree::new();
    fs::create_dir_all(dir.path("private")).unwrap();
    fs::write(dir.path("private/secret.txt"), "needle\n").unwrap();
    fs::write(dir.path("accessible.txt"), "needle\n").unwrap();
    let hidden = dir.path("private");
    fs::set_permissions(&hidden, fs::Permissions::from_mode(0o000)).unwrap();
    let blocked = fs::read_dir(&hidden).is_err();
    let root = dir.root();
    let ls = search::Search::new(root.clone(), search::Operation::Find);
    let listing = call(&ls, json!({"depth":3,"glob":"**/*.txt"})).await;

    let grep = search::Search::new(root.clone(), search::Operation::Grep);
    let found = call(&grep, json!({"pattern":"needle","mode":"files"})).await;

    fs::set_permissions(&hidden, fs::Permissions::from_mode(0o700)).unwrap();
    if blocked {
        assert!(
            listing.content.contains("incomplete walk"),
            "{}",
            listing.content
        );
        assert!(
            found.content.contains("incomplete walk"),
            "{}",
            found.content
        );
    }
    let restricted = dir.path("restricted.txt");
    fs::write(&restricted, "needle\n").unwrap();
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o000)).unwrap();
    let unreadable = fs::File::open(&restricted).is_err();
    let found = call(&grep, json!({"pattern":"needle","mode":"files"})).await;

    let content = call(&grep, json!({"pattern":"needle","mode":"content"})).await;
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o600)).unwrap();
    if unreadable {
        assert!(
            found.content.contains("incomplete walk"),
            "{}",
            found.content
        );
        assert!(content.content.contains("incomplete walk"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn missing_versus_inaccessible_gitignore_are_distinguished() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Tree::new();
    fs::write(dir.path("hit.txt"), "needle\n").unwrap();
    let ignore = dir.path(".gitignore");
    fs::write(&ignore, "hit.txt\n").unwrap();
    let root = dir.root();
    let grep = search::Search::new(root, search::Operation::Grep);
    let filtered = call(
        &grep,
        json!({"pattern":"needle","gitignore":true,"mode":"files"}),
    )
    .await;
    assert_eq!(filtered.content, "");
    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o000)).unwrap();
    let inaccessible = fs::File::open(&ignore).is_err();
    let files = call(
        &grep,
        json!({"pattern":"needle","gitignore":true,"mode":"files"}),
    )
    .await;
    let content = call(
        &grep,
        json!({"pattern":"needle","gitignore":true,"mode":"content"}),
    )
    .await;
    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o600)).unwrap();
    if inaccessible {
        assert!(
            files.content.contains("incomplete gitignore"),
            "{}",
            files.content
        );
        assert!(content.content.contains("incomplete gitignore"));
    }
    fs::remove_file(ignore).unwrap();
    let absent = call(
        &grep,
        json!({"pattern":"needle","gitignore":true,"mode":"files"}),
    )
    .await;
    assert_eq!(absent.content, "hit.txt\n");
}

#[tokio::test]
async fn grep_preserves_long_matching_line_location_and_pages_beyond_fifty() {
    let tree = Tree::new();
    let long = format!("needle{}\n", "x".repeat(100_000));
    fs::write(tree.path("long.txt"), long).unwrap();
    fs::write(tree.path("many.txt"), "needle\n".repeat(60)).unwrap();
    fs::write(tree.path("other.txt"), "needle\n".repeat(3)).unwrap();
    let grep = search::Search::new(tree.root(), search::Operation::Grep);
    let one = call(
        &grep,
        json!({"path":"long.txt","pattern":"needle","limit":200}),
    )
    .await;
    assert!(one.is_error, "first unrenderable primary hit must fail");
    assert!(
        one.content
            .contains("grep hit/context exceeds result ceiling"),
        "{}",
        one.content
    );

    let all = call(
        &grep,
        json!({"path":"many.txt","pattern":"needle","limit":200,"mode":"content"}),
    )
    .await;
    assert!(!all.is_error, "{}", all.content);
    assert_eq!(all.content.matches(":needle\n").count(), 60);
    assert!(
        all.content.contains("many.txt:60:needle"),
        "{}",
        all.content
    );
    assert!(!all.content.contains("truncated at 50 hits"));
}

#[tokio::test]
async fn both_native_file_discovery_tools_hide_only_exact_staging_names() {
    let tree = Tree::new();
    fs::write(
        tree.path(".dotlink-321-0123456789abcdef.tmp"),
        "STAGING_SECRET",
    )
    .unwrap();
    fs::write(tree.path(".dotlink-notes.tmp"), "normal").unwrap();
    fs::write(
        tree.path(".aw-stage-321-0123456789abcdef.tmp"),
        "STAGING_SECRET",
    )
    .unwrap();
    fs::write(tree.path("ordinary.txt"), "STAGING_SECRET").unwrap();
    let root = tree.root();
    let ls = search::Search::new(root.clone(), search::Operation::Find);
    let listed = call(&ls, json!({"glob":"**/*","kind":"file","hidden":true})).await;
    assert!(listed.content.lines().any(|line| line == "f  ordinary.txt"));
    assert!(listed
        .content
        .lines()
        .any(|line| line == "f  .dotlink-notes.tmp"));
    assert!(!listed.content.contains("0123456789abcdef"));
    let grep = search::Search::new(root, search::Operation::Grep);
    let result = call(&grep, json!({"pattern":"STAGING_SECRET","mode":"files"})).await;
    assert_eq!(result.content, "ordinary.txt\n");
}

#[tokio::test]
async fn read_truncation_marker_reports_last_returned_line() {
    let tree = Tree::new();
    fs::write(tree.path("lines.txt"), "one\ntwo\n").unwrap();
    let read = text::Text::new(tree.root(), text::Operation::Read);
    let result = call(&read, json!({"path":"lines.txt","offset":1,"limit":1})).await;
    assert!(
        result.content.contains("truncated after line 1"),
        "{}",
        result.content
    );
    assert!(
        !result.content.contains("truncated after line 2"),
        "{}",
        result.content
    );
}

#[tokio::test]
async fn grep_abbreviated_hit_and_page_limit_both_remain_visible() {
    let tree = Tree::new();
    let long = format!("needle{}\n", "x".repeat(70_000));
    fs::write(tree.path("hits.txt"), format!("{long}needle second\n")).unwrap();
    let grep = search::Search::new(tree.root(), search::Operation::Grep);
    let out = call(
        &grep,
        json!({"path":"hits.txt","pattern":"needle",
        "limit":1,"mode":"content"}),
    )
    .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.truncated);
    assert!(out.content.contains("hits.txt:1:"), "{}", out.content);
    assert!(
        out.content.contains(&"x".repeat(70_000)),
        "the complete primary hit must not be replaced with a placeholder"
    );
    assert!(!out.content.contains("matching line omitted"));
    assert!(out.content.contains("more offset=1"), "{}", out.content);
}

#[cfg(unix)]
#[test]
fn privileged_mode_replacement_is_denied() {
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::new();
    fs::write(tree.path("privileged.sh"), "old").unwrap();
    fs::set_permissions(
        tree.path("privileged.sh"),
        fs::Permissions::from_mode(0o4755),
    )
    .unwrap();
    let root = tree.root();
    let _lock = root.lock_mutations(&["privileged.sh".to_owned()]).unwrap();
    let planned = root.stage_update("privileged.sh", b"old", b"new", &|| false);
    assert!(
        planned.is_err(),
        "must not create privileged executable bytes"
    );
    assert_eq!(fs::read(tree.path("privileged.sh")).unwrap(), b"old");
}

#[cfg(windows)]
#[test]
fn windows_native_shell_identity_is_pwsh() {
    let tree = Tree::new();
    let shell = super::shell::PowerShell::new(
        tree.0.clone(),
        PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
    )
    .unwrap();
    assert_eq!(shell.spec().name, "powershell");
    let modern = super::shell::PowerShell::new(
        tree.0.clone(),
        PathBuf::from("C:/Program Files/PowerShell/7/pwsh.exe"),
    )
    .unwrap();
    assert_eq!(modern.spec().name, "pwsh");
    assert!(super::shell::PowerShell::new(
        tree.0.clone(),
        PathBuf::from("C:/Windows/System32/cmd.exe")
    )
    .is_err());
}

#[tokio::test]
async fn private_mutation_stages_are_not_model_selectable_by_exact_path() {
    let tree = Tree::new();
    let root = tree.root();
    for stem in [
        ".aw-stage-5-0123456789abcdef.tmp",
        "parent/.aw-trash-9-0123456789abcdef.tmp",
        ".aw-backup-99-0123456789abcdef.tmp",
        "tmp/.dotlink-5-0123456789abcdef.tmp",
    ] {
        let read = text::Text::new(root.clone(), text::Operation::Read);
        let write = text::Text::new(root.clone(), text::Operation::Write);
        let bread = binary::Binary::new(root.clone(), binary::Operation::Read);
        let bwrite = binary::Binary::new(root.clone(), binary::Operation::Write);
        let edit = edit::Edit::new(root.clone());
        let patch = patch::Patch::new(root.clone());
        let grep = search::Search::new(root.clone(), search::Operation::Grep);
        for (tool, arg) in [
            (&read as &dyn Tool, json!({"path":stem})),
            (&write as &dyn Tool, json!({"path":stem,"content":"x"})),
            (&bread as &dyn Tool, json!({"path":stem})),
            (
                &bwrite as &dyn Tool,
                json!({"path":stem,"data":"78","encoding":"hex"}),
            ),
            (&edit as &dyn Tool, json!({"path":stem,"edits":[["a","b"]]})),
            (&grep as &dyn Tool, json!({"path":stem,"pattern":"a"})),
        ] {
            assert!(
                tool.validate(&arg).is_err(),
                "{} accepted protected path {stem}",
                tool.spec().name
            );
        }
        let payload = format!("*** Begin Patch\n*** Add File: {stem}\n+x\n*** End Patch");
        let result = patch.execute(context(), &json!({"patch":payload})).await;
        assert!(
            result.is_err() || result.as_ref().is_ok_and(|out| out.is_error),
            "patch accepted protected path {stem}"
        );
    }
    assert!(aw_tools_core::filesystem::is_protected_path(
        ".aw-stage-1-0123456789abcdef.tmp"
    ));
    assert!(!aw_tools_core::filesystem::is_protected_path(
        ".aw-stage-ordinary-user.txt"
    ));
}
#[tokio::test]
async fn canonical_text_write_rejects_binary_controls_without_creating_file() {
    let tree = Tree::new();
    let tool = text::Text::new(tree.root(), text::Operation::Write);
    let arguments = json!({"path":"fresh/a.txt",
        "content":"hello\u{0000}world"});
    match tool.execute(context(), &arguments).await {
        Err(aw_agent_core::Error::Denied(_)) => {}
        Ok(output) if output.is_error => {}
        other => panic!("binary control write unexpectedly accepted: {other:?}"),
    }
    assert!(!tree.path("fresh").exists());
}
#[tokio::test]
async fn sealed_snapshot_edit_noop_range_may_overlap_real_change() {
    let tree = Tree::new();
    fs::write(tree.path("sample.txt"), "abc").unwrap();
    let edit = edit::Edit::new(tree.root());
    let applied = call(
        &edit,
        json!({"path":"sample.txt","edits":[
            ["ab","ab"],["b","B"]
        ]}),
    )
    .await;
    assert!(!applied.is_error, "{}", applied.content);
    assert_eq!(applied.content, "ok");
    assert_eq!(fs::read_to_string(tree.path("sample.txt")).unwrap(), "aBc");
}
#[tokio::test]
async fn snapshot_edit_tuples_are_independent_order_free_and_one_file() {
    let tree = Tree::new();
    fs::write(tree.path("f.txt"), "alpha beta gamma").unwrap();
    let tool = edit::Edit::new(tree.root());
    let result = call(
        &tool,
        json!({"path":"f.txt","edits":[
            ["alpha","beta"],["beta","X"],["gamma","G"]
        ]}),
    )
    .await;
    assert_eq!(result.content, "ok");
    assert_eq!(fs::read_to_string(tree.path("f.txt")).unwrap(), "beta X G");
    let denied = call(
        &tool,
        json!({"path":"f.txt","edits":[
            ["beta","1"],["beta","2"]
        ]}),
    )
    .await;
    assert!(denied.is_error);
    assert!(denied.content.contains("overlap"));
    assert_eq!(fs::read_to_string(tree.path("f.txt")).unwrap(), "beta X G");
    let no_op = call(&tool, json!({"path":"f.txt","edits":[["X","X"]]})).await;
    assert_eq!(no_op.content, "unchanged");
    let unknown = call(&tool, json!({"path":"f.txt","edits":[["MISSING","x"]]})).await;
    assert!(unknown.is_error);
}
#[tokio::test]
async fn singular_edit_is_canonical_and_exact() {
    let tree = Tree::new();
    fs::write(tree.path("file.txt"), "a a a").unwrap();
    let edit = edit::Edit::new(tree.root());
    let ambiguous = call(&edit, json!({"path":"file.txt","old":"a","new":"b"})).await;
    assert!(ambiguous.is_error);
    assert!(
        ambiguous.content.contains("3 matches"),
        "{}",
        ambiguous.content
    );
    assert_eq!(fs::read_to_string(tree.path("file.txt")).unwrap(), "a a a");
    let applied = call(
        &edit,
        json!({"path":"file.txt","old":"a","new":"b",
        "all":true}),
    )
    .await;
    assert_eq!(applied.content, "ok");
    assert_eq!(fs::read_to_string(tree.path("file.txt")).unwrap(), "b b b");
    let noop = call(
        &edit,
        json!({"path":"file.txt","old":"b","new":"b",
        "all":true}),
    )
    .await;
    assert_eq!(noop.content, "unchanged");
    assert!(edit
        .validate(&json!({"path":"file.txt","old":"","new":"x"}))
        .is_err());
}
#[tokio::test]
async fn canonical_edit_schema_and_legacy_input_aliases_both_work() {
    let tree = Tree::new();
    fs::write(tree.path("a.txt"), "one two").unwrap();
    let tool = edit::Edit::new(tree.root());
    let new = call(
        &tool,
        json!({"edits":[{"path":"a.txt","old":"one",
        "new":"ONE","all":false}]}),
    )
    .await;
    assert_eq!(new.content, "ok 1 edits 1 files");
    let old = call(
        &tool,
        json!({"edits":[{"path":"a.txt","old_text":"two",
        "new_text":"TWO","replace_all":false}]}),
    )
    .await;
    assert_eq!(old.content, "ok 1 edits 1 files");
    assert_eq!(fs::read_to_string(tree.path("a.txt")).unwrap(), "ONE TWO");
}
#[tokio::test]
async fn shared_edit_limits_and_bounded_multi_file_receipts() {
    let tree = Tree::new();
    let mut edits = Vec::new();
    for file in 0..64 {
        let path = format!("file{file:03}.txt");
        let old = (0..8)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        fs::write(tree.path(&path), old).unwrap();
        for index in 0..8 {
            edits.push(json!({"path":path,"old_text":format!("a{index}"),
                "new_text":format!("b{index}")}));
        }
    }
    assert_eq!(edits.len(), 512);
    let tool = edit::Edit::new(tree.root());
    let reply = call(&tool, json!({"edits":edits})).await;
    assert!(!reply.is_error, "{}", reply.content);
    assert_eq!(reply.content, "ok 512 edits 64 files");
    assert_eq!(
        fs::read_to_string(tree.path("file000.txt")).unwrap(),
        (0..8)
            .map(|i| format!("b{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let too_many = (0..513)
        .map(|_| json!({"old_text":"a","new_text":"b"}))
        .collect::<Vec<_>>();
    assert!(tool
        .validate(&json!({"path":"file000.txt","edits":too_many}))
        .is_err());
}

#[tokio::test]
async fn codex_patch_eof_and_mixed_line_endings_adapt_from_dotlink() {
    let tree = Tree::new();
    fs::write(tree.path("mixed.txt"), b"head\r\nold\ntail\r\n").unwrap();
    fs::write(tree.path("repeat.txt"), b"same\nsame\n").unwrap();
    let patch = patch::Patch::new(tree.root());
    let crlf=call(&patch,json!({"patch":
        "*** Begin Patch\n*** Update File: mixed.txt\n@@\n head\n-old\n+new\n tail\n*** End Patch\n"
    })).await;
    assert!(!crlf.is_error, "{}", crlf.content);
    assert_eq!(
        fs::read(tree.path("mixed.txt")).unwrap(),
        b"head\r\nnew\ntail\r\n"
    );
    let eof=call(&patch,json!({"patch":
        "*** Begin Patch\n*** Update File: repeat.txt\n@@\n-same\n+last\n*** End of File\n*** End Patch\n"
    })).await;
    assert!(!eof.is_error, "{}", eof.content);
    assert_eq!(fs::read(tree.path("repeat.txt")).unwrap(), b"same\nlast\n");
    let add = call(
        &patch,
        json!({"patch":
            "*** Begin Patch\r\n*** Add File: crlf.txt\r\n+one\r\n+\r\n+two\r\n*** End Patch\r\n"
        }),
    )
    .await;
    assert!(!add.is_error, "{}", add.content);
    assert_eq!(
        fs::read(tree.path("crlf.txt")).unwrap(),
        b"one\r\n\r\ntwo\r\n"
    );
}

#[tokio::test]
async fn binary_streamed_multi_range_handles_insert_delete_and_sha_precondition() {
    use sha2::Digest;
    let tree = Tree::new();
    let mut original = vec![b'A'; 4 * 1024 * 1024];
    original[10..14].copy_from_slice(b"WXYZ");
    fs::write(tree.path("blob.bin"), &original).unwrap();
    let sha = format!("{:x}", sha2::Sha256::digest(&original));
    let tool = binary::Binary::new(tree.root(), binary::Operation::Edit);
    let out = call(
        &tool,
        json!({"path":"blob.bin","sha256":sha,
            "edits":[
                {"offset":10,"length":4,"data":"QkM="},
                {"offset":1024,"length":0,"data":"WA=="},
                {"offset":2000,"length":10,"data":""}
            ]
        }),
    )
    .await;
    assert!(!out.is_error, "{}", out.content);
    assert_eq!(out.content, "ok");
    let result = fs::read(tree.path("blob.bin")).unwrap();
    assert_eq!(&result[10..12], b"BC");
    assert_eq!(result[1022], b'X');
    assert_eq!(result[1023], b'A');
    assert_eq!(result.len(), original.len() - 4 + 2 + 1 - 10);
    let stale = call(
        &tool,
        json!({"path":"blob.bin","sha256":sha,
        "edits":[{"offset":5,"length":1,"data":"WA=="}]}),
    )
    .await;
    assert!(stale.is_error);
}

#[tokio::test]
async fn search_incomplete_reasons_and_ambiguous_path_labels_are_explicit() {
    let tree = Tree::new();
    fs::write(tree.path("foo:bar.txt"), "REVIEW\n").unwrap();
    let grep = search::Search::new(tree.root(), search::Operation::Grep);
    let page = call(
        &grep,
        json!({"path":".","pattern":"REVIEW","mode":"content"}),
    )
    .await;
    assert!(!page.is_error, "{}", page.content);
    assert!(
        page.content.contains("\"foo:bar.txt\":1:REVIEW"),
        "{}",
        page.content
    );
    let files = call(&grep, json!({"path":".","pattern":"REVIEW","mode":"files"})).await;
    assert_eq!(files.content, r#""foo:bar.txt""#.to_string() + "\n");
    assert!(!files.truncated);
}

#[tokio::test]
async fn directory_grep_reports_oversize_skipped_files_as_incomplete() {
    let tree = Tree::new();
    let mut source = vec![b'x'; 1024 * 1024 + 2];
    source.extend_from_slice(
        b"
REVIEW_NEEDLE
",
    );
    fs::write(tree.path("oversize.txt"), source).unwrap();
    let grep = search::Search::new(tree.root(), search::Operation::Grep);
    let result = call(
        &grep,
        json!({"path":".","pattern":"REVIEW_NEEDLE",
        "mode":"files","limit":100}),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert!(result.truncated);
    assert!(
        result.content.contains("incomplete budget"),
        "{}",
        result.content
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn grep_retains_hit_when_long_path_context_exceeds_output_budget() {
    let tree = Tree::new();
    let mut dir = String::new();
    for number in 0..17 {
        if !dir.is_empty() {
            dir.push('/');
        }
        dir.push_str(&format!("{number:03}{}", "z".repeat(225)));
    }
    fs::create_dir_all(tree.path(&dir)).unwrap();
    let path = format!("{dir}/hit.txt");
    let text = (0..17)
        .map(|line| {
            format!(
                "{}{}
",
                "a".repeat(2000),
                if line == 8 { "NEEDLE" } else { "" }
            )
        })
        .collect::<String>();
    fs::write(tree.path(&path), text).unwrap();
    let grep = search::Search::new(tree.root(), search::Operation::Grep);
    let result = call(
        &grep,
        json!({"path":path,"pattern":"NEEDLE",
        "before":8,"after":8,"limit":1}),
    )
    .await;
    assert!(result.is_error);
    assert!(
        result
            .content
            .contains("grep hit/context exceeds result ceiling"),
        "{}",
        result.content
    );
}

#[tokio::test]
async fn lifecycle_copy_exact_files_directories_and_symlinks_without_overwrite() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("src/nested")).unwrap();
    fs::write(tree.path("src/nested/code.rs"), b"canonical").unwrap();
    let root = tree.root();
    let cp = lifecycle::Lifecycle::new(root.clone(), lifecycle::Operation::Copy);
    let copied = call(
        &cp,
        json!({"from":"src/nested/code.rs",
        "to":"copies/final.rs"}),
    )
    .await;
    assert_eq!(copied.content, "ok");
    assert_eq!(
        fs::read(tree.path("copies/final.rs")).unwrap(),
        b"canonical"
    );
    let duplicate = call(
        &cp,
        json!({"from":"src/nested/code.rs",
        "to":"copies/final.rs"}),
    )
    .await;
    assert!(duplicate.is_error, "must not overwrite");
    assert_eq!(
        fs::read(tree.path("copies/final.rs")).unwrap(),
        b"canonical"
    );
    assert!(
        cp.validate(&json!({"from":"src","to":"src/copy","recursive":true}))
            .is_err(),
        "cannot copy source directory inside itself"
    );
    let denied = call(&cp, json!({"from":"src","to":"src_copy","recursive":false})).await;
    assert!(denied.is_error, "directory copy needs explicit recursive");
    let copied = call(&cp, json!({"from":"src","to":"src_copy","recursive":true})).await;
    assert_eq!(copied.content, "ok");
    assert_eq!(
        fs::read(tree.path("src_copy/nested/code.rs")).unwrap(),
        b"canonical"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            "../../src/nested/code.rs",
            tree.path("src_copy/nested/link"),
        )
        .unwrap();
        let result = call(
            &cp,
            json!({"from":"src_copy/nested/link",
            "to":"copies/link"}),
        )
        .await;
        assert_eq!(result.content, "ok");
        assert!(fs::symlink_metadata(tree.path("copies/link"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_link(tree.path("copies/link")).unwrap(),
            std::path::Path::new("../../src/nested/code.rs")
        );
    }
}
#[tokio::test]
async fn lifecycle_move_is_exact_atomic_rename_never_copy_delete() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("src/deep")).unwrap();
    fs::write(tree.path("src/deep/old.txt"), "payload").unwrap();
    fs::create_dir_all(tree.path("new")).unwrap();
    let mv = lifecycle::Lifecycle::new(tree.root(), lifecycle::Operation::Move);
    let result = call(
        &mv,
        json!({"from":"src/deep/old.txt",
        "to":"new/renamed.txt"}),
    )
    .await;
    assert_eq!(result.content, "ok");
    assert!(!tree.path("src/deep/old.txt").exists());
    assert_eq!(
        fs::read_to_string(tree.path("new/renamed.txt")).unwrap(),
        "payload"
    );
    assert!(mv
        .validate(&json!({"from":"src","to":"src/inside"}))
        .is_err());
    let denied = call(&mv, json!({"from":"src","to":"new"})).await;
    assert!(denied.is_error, "no existing destination overwrite");
    let missing_parent = call(
        &mv,
        json!({"from":"src",
        "to":"not-created/renamed-src"}),
    )
    .await;
    assert!(missing_parent.is_error);
    assert!(!tree.path("not-created").exists());
    assert!(tree.path("src").exists());
    let renamed = call(&mv, json!({"from":"src","to":"renamed_src"})).await;
    assert_eq!(renamed.content, "ok");
    assert!(!tree.path("src").exists());
    assert!(tree.path("renamed_src/deep").is_dir());
    assert_eq!(
        call(&mv, json!({"from":"renamed_src","to":"renamed_src"}))
            .await
            .content,
        "unchanged"
    );
}
#[cfg(unix)]
#[test]
fn lifecycle_partial_cancel_and_post_rename_cleanup_have_distinct_states() {
    use aw_tools_core::filesystem::lifecycle::RemoveOutcome;
    let tree = Tree::new();
    fs::write(tree.path("a.txt"), "a").unwrap();
    fs::write(tree.path("b.txt"), "b").unwrap();
    let root = tree.root();
    let cancel_after_first_cleanup = || {
        let a_gone = !tree.path("a.txt").exists();
        let pending = fs::read_dir(&tree.0).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".aw-trash-")
        });
        a_gone && !pending
    };
    let _guard = root
        .lock_mutations(&["a.txt".into(), "b.txt".into()])
        .unwrap();
    let result = root
        .remove_paths(
            &["a.txt".into(), "b.txt".into()],
            false,
            &cancel_after_first_cleanup,
        )
        .unwrap();
    assert_eq!(
        result,
        RemoveOutcome::Partial {
            ok: 1,
            total: 2,
            at: 2,
            reason: "cancelled"
        }
    );
    assert!(!tree.path("a.txt").exists());
    assert!(tree.path("b.txt").exists());
    drop(_guard);

    fs::write(tree.path("c.txt"), "c").unwrap();
    let _guard = root.lock_mutations(&["c.txt".into()]).unwrap();
    let cancel_after_namespace_rename = || !tree.path("c.txt").exists();
    let result = root
        .remove_paths(&["c.txt".into()], false, &cancel_after_namespace_rename)
        .unwrap();
    assert_eq!(
        result,
        RemoveOutcome::CleanupPending {
            ok: 1,
            total: 1,
            at: 1
        }
    );
    assert!(!tree.path("c.txt").exists());
    assert!(fs::read_dir(&tree.0).unwrap().flatten().any(|entry| entry
        .file_name()
        .to_string_lossy()
        .starts_with(".aw-trash-")));
}
#[cfg(unix)]
#[tokio::test]
async fn lifecycle_delete_refuses_shared_hardlink_targets_before_unlink() {
    let tree = Tree::new();
    fs::write(tree.path("original"), b"safe").unwrap();
    fs::hard_link(tree.path("original"), tree.path("alias")).unwrap();
    let tool = lifecycle::Lifecycle::new(tree.root(), lifecycle::Operation::Remove);
    let result = call(&tool, json!({"paths":["original","alias"]})).await;
    assert!(result.is_error);
    assert_eq!(fs::read(tree.path("original")).unwrap(), b"safe");
    assert_eq!(fs::read(tree.path("alias")).unwrap(), b"safe");
}
#[tokio::test]
async fn lifecycle_delete_preflights_all_roots_and_does_not_follow_links() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("nested/deep")).unwrap();
    fs::write(tree.path("nested/deep/file.txt"), b"secret").unwrap();
    fs::write(tree.path("keep.txt"), b"keep").unwrap();
    fs::write(tree.path("other.txt"), b"other").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("../keep.txt", tree.path("nested/linked")).unwrap();
    let rm = lifecycle::Lifecycle::new(tree.root(), lifecycle::Operation::Remove);
    let rejected = call(&rm, json!({"paths":["other.txt","nested"]})).await;
    assert!(rejected.is_error, "nonempty directory requires recursive");
    assert!(
        tree.path("other.txt").exists(),
        "no partial preflight effect"
    );
    assert!(
        rm.validate(&json!({"paths":["nested","nested/deep"],"recursive":true}))
            .is_err(),
        "ancestor overlaps are forbidden"
    );
    let removed = call(
        &rm,
        json!({"paths":["other.txt","nested"],"recursive":true}),
    )
    .await;
    assert_eq!(removed.content, "ok 2");
    assert!(!tree.path("nested").exists());
    assert!(!tree.path("other.txt").exists());
    assert_eq!(fs::read(tree.path("keep.txt")).unwrap(), b"keep");
    let absent = call(&rm, json!({"paths":["other.txt"]})).await;
    assert!(absent.is_error, "absent path is not silent success");
    let left = fs::read_dir(&tree.0)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert!(!left.iter().any(|name| name.starts_with(".aw-trash-")));
}

#[tokio::test]
async fn sealed_grep_context_and_copy_recursive_default() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("source/sub")).unwrap();
    fs::write(tree.path("source/sub/f.rs"), "before\nmatch\nafter\n").unwrap();
    let find = search::Search::new(tree.root(), search::Operation::Grep);
    let searched = call(
        &find,
        json!({"path":"source","pattern":"match",
        "context":1}),
    )
    .await;
    assert!(!searched.is_error, "{}", searched.content);
    assert!(searched.content.contains("before"), "{}", searched.content);
    assert!(searched.content.contains("after"), "{}", searched.content);
    let cp = lifecycle::Lifecycle::new(tree.root(), lifecycle::Operation::Copy);
    let copied = call(&cp, json!({"from":"source","to":"copied"})).await;
    assert_eq!(copied.content, "ok");
    assert_eq!(
        fs::read_to_string(tree.path("copied/sub/f.rs")).unwrap(),
        "before\nmatch\nafter\n"
    );
    assert!(cp.spec().parameters["properties"]
        .get("recursive")
        .is_none());
    assert!(find.spec().parameters["properties"].get("before").is_none());
    assert!(find.spec().parameters["properties"].get("after").is_none());
    let patch = patch::Patch::new(tree.root());
    assert!(patch.spec().parameters["properties"]
        .get("best_effort")
        .is_none());
}
#[test]
fn dl150_schema_and_compact_rendering_budgets_are_measured() {
    let tree = Tree::new();
    let root = tree.root();
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(search::Search::new(root.clone(), search::Operation::Ls)),
        Box::new(search::Search::new(root.clone(), search::Operation::Find)),
        Box::new(search::Search::new(root.clone(), search::Operation::Grep)),
        Box::new(text::Text::new(root.clone(), text::Operation::Read)),
        Box::new(text::Text::new(root.clone(), text::Operation::Write)),
        Box::new(edit::Edit::new(root.clone())),
        Box::new(patch::Patch::new(root.clone())),
        Box::new(lifecycle::Lifecycle::new(
            root.clone(),
            lifecycle::Operation::Copy,
        )),
        Box::new(lifecycle::Lifecycle::new(
            root.clone(),
            lifecycle::Operation::Move,
        )),
        Box::new(lifecycle::Lifecycle::new(
            root.clone(),
            lifecycle::Operation::Remove,
        )),
        Box::new(binary::Binary::new(root.clone(), binary::Operation::Read)),
        Box::new(binary::Binary::new(root.clone(), binary::Operation::Write)),
        Box::new(binary::Binary::new(root, binary::Operation::Edit)),
    ];
    let mut total = 0usize;
    for tool in tools {
        let spec = tool.spec();
        let bytes = serde_json::to_vec(&spec).unwrap().len();
        println!("schema {} {}", spec.name, bytes);
        total += bytes;
    }
    println!("schema total tools=13 bytes={total}");
    assert!(total < 16 * 1024, "unexpected schema bloat: {total}");
    // Typical 100-file listing, equivalent semantic information.
    let mut compact = String::new();
    let mut verbose = Vec::new();
    for i in 0..100 {
        let path = format!("src/module{i:03}.rs");
        compact.push_str(&format!("f  {path}\n"));
        verbose.push(json!({"path":path,"kind":"file","size":1234}));
    }
    let legacy = json!({"entries":verbose,"truncated":false,
        "scan_incomplete":false,"incomplete_reason":null,
        "incomplete_reasons":[]})
    .to_string();
    println!(
        "listing-100 compact={} legacy_json={}",
        compact.len(),
        legacy.len()
    );
    assert!(
        compact.len() * 2 < legacy.len(),
        "compact results must be materially smaller"
    );
}
#[test]
fn read_write_full_modes_are_backwards_compatible_but_never_widen() {
    use aw_tools_core::filesystem::access::{Access, Action};
    assert_eq!("rw".parse::<ToolMode>().unwrap(), ToolMode::Workspace);
    assert_eq!(
        "workspace".parse::<ToolMode>().unwrap(),
        ToolMode::Workspace
    );
    assert_eq!("full".parse::<ToolMode>().unwrap(), ToolMode::Trusted);
    assert_eq!("trusted".parse::<ToolMode>().unwrap(), ToolMode::Trusted);
    assert_eq!(ToolMode::Workspace.to_string(), "rw");
    assert_eq!(ToolMode::Trusted.to_string(), "full");
    assert_eq!(ToolMode::Workspace.storage_label(), "workspace");
    assert_eq!(ToolMode::Trusted.storage_label(), "trusted");
    assert!(!Access::ReadOnly.permits(Action::Remove));
    assert!(Access::ReadWrite.permits(Action::Copy));
    assert!(!Access::ReadWrite.permits(Action::Shell));
    assert!(Access::Full.permits(Action::Shell));
}

#[tokio::test]
async fn canonical_discovery_and_grep_page_offsets_resume_without_repeating() {
    let tree = Tree::new();
    fs::write(
        tree.path("a.txt"),
        "needle a
",
    )
    .unwrap();
    fs::write(
        tree.path("b.txt"),
        "needle b
",
    )
    .unwrap();
    fs::write(
        tree.path("c.txt"),
        "needle c
",
    )
    .unwrap();
    let find = search::Search::new(tree.root(), search::Operation::Find);
    let first = call(&find, json!({"glob":"*.txt","limit":1})).await;
    assert_eq!(
        first.content,
        "f  a.txt
more offset=1
"
    );
    let second = call(&find, json!({"glob":"*.txt","offset":1,"limit":1})).await;
    assert_eq!(
        second.content,
        "f  b.txt
more offset=2
"
    );
    let last = call(&find, json!({"glob":"*.txt","offset":2,"limit":1})).await;
    assert_eq!(
        last.content,
        "f  c.txt
"
    );
    let grep = search::Search::new(tree.root(), search::Operation::Grep);
    let a = call(
        &grep,
        json!({"pattern":"needle","mode":"content",
        "limit":1}),
    )
    .await;
    assert!(a.content.contains("a.txt:1:"), "{}", a.content);
    assert!(
        a.content.ends_with(
            "more offset=1
"
        ),
        "{}",
        a.content
    );
    let b = call(
        &grep,
        json!({"pattern":"needle","mode":"content",
        "offset":1,"limit":1}),
    )
    .await;
    assert!(b.content.contains("b.txt:1:"), "{}", b.content);
    assert!(
        b.content.ends_with(
            "more offset=2
"
        ),
        "{}",
        b.content
    );
}

#[tokio::test]
async fn grep_has_independent_paging_and_incomplete_coverage() {
    let tree = Tree::new();
    fs::write(tree.path("0_oversized.txt"), vec![b'x'; 1024 * 1024 + 5]).unwrap();
    fs::write(
        tree.path("a.txt"),
        "needle a
",
    )
    .unwrap();
    fs::write(
        tree.path("b.txt"),
        "needle b
",
    )
    .unwrap();
    let grep = search::Search::new(tree.root(), search::Operation::Grep);
    let first = call(
        &grep,
        json!({"pattern":"needle","mode":"files",
        "limit":1}),
    )
    .await;
    assert_eq!(
        first.content,
        "a.txt
more offset=1 incomplete=budget
"
    );
    assert!(first.truncated);
}

#[cfg(unix)]
#[tokio::test]
async fn sealed_atomic_parent_suffix_for_write_and_copy() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("source/sub")).unwrap();
    fs::write(tree.path("source/sub/f.txt"), "data").unwrap();
    let root = tree.root();
    let write = text::Text::new(root.clone(), text::Operation::Write);
    let written = call(&write, json!({"path":"fresh/deep/a.txt","content":"ok"})).await;
    assert_eq!(written.content, "ok");
    assert_eq!(fs::read(tree.path("fresh/deep/a.txt")).unwrap(), b"ok");
    let cp = lifecycle::Lifecycle::new(root.clone(), lifecycle::Operation::Copy);
    let result = call(&cp, json!({"from":"source","to":"copy/new/deep"})).await;
    assert_eq!(result.content, "ok");
    assert_eq!(
        fs::read(tree.path("copy/new/deep/sub/f.txt")).unwrap(),
        b"data"
    );
    let raw = root.copy_path("source", "failed/new/tree", true, &|| {
        fs::read_dir(&tree.0).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".aw-stage-")
        })
    });
    assert!(raw.is_err(), "cancellation after private staging must fail");
    assert!(
        !tree.path("failed").exists(),
        "no published new parent suffix"
    );
    assert!(!fs::read_dir(&tree.0).unwrap().flatten().any(|entry| entry
        .file_name()
        .to_string_lossy()
        .starts_with(".aw-stage-")));
}

#[cfg(unix)]
#[tokio::test]
async fn unix_non_utf8_filenames_are_omitted_with_exact_path_encoding_warning() {
    use std::os::unix::ffi::OsStrExt;
    let tree = Tree::new();
    fs::write(
        tree.path("safe.txt"),
        "needle
",
    )
    .unwrap();
    let invalid = std::ffi::OsStr::from_bytes(b"bad\xff.txt");
    fs::write(
        tree.0.join(invalid),
        "needle
",
    )
    .unwrap();
    let root = tree.root();
    let find = search::Search::new(root.clone(), search::Operation::Find);
    let entries = call(&find, json!({"path":"."})).await;
    assert!(!entries.is_error, "{}", entries.content);
    assert!(entries.content.contains(
        "f  safe.txt
"
    ));
    assert!(
        entries.content.contains("incomplete path_encoding"),
        "{}",
        entries.content
    );
    assert!(
        !entries.content.contains("\u{fffd}"),
        "lossy replacement characters must not become reusable paths"
    );
    let grep = search::Search::new(root, search::Operation::Grep);
    let result = call(&grep, json!({"pattern":"needle","mode":"files"})).await;
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.contains(
        "safe.txt
"
    ));
    assert!(
        result.content.contains("incomplete path_encoding"),
        "{}",
        result.content
    );
}
