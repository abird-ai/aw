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
async fn ls_replaces_find_and_grep_has_file_count_content_modes() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("src/deep")).unwrap();
    fs::write(dir.path("src/deep/code.rs"), "TEST\nTEST\n").unwrap();
    fs::write(dir.path("src/deep/other.txt"), "TEST\n").unwrap();
    fs::write(dir.path(".hidden"), "TEST\n").unwrap();
    let root = dir.root();
    let ls = search::Search::new(root.clone(), search::Operation::Ls);
    let result = call(&ls, json!({"pattern":"**/*.rs","kind":"file"})).await;
    assert!(!result.is_error);
    let files: Value = serde_json::from_str(&result.content).unwrap();
    assert!(files["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["path"] == "src/deep/code.rs"));
    let grep = search::Search::new(root, search::Operation::Grep);
    let counted = call(
        &grep,
        json!({"pattern":"TEST","glob":"**/*.rs","output_mode":"count"}),
    )
    .await;
    let data: Value = serde_json::from_str(&counted.content).unwrap();
    assert_eq!(data["page_total"], 2);
    let filenames = call(
        &grep,
        json!({"pattern":"TEST","glob":"**/*.rs","output_mode":"files"}),
    )
    .await;
    let files: Value = serde_json::from_str(&filenames.content).unwrap();
    assert_eq!(files["files"], json!(["src/deep/code.rs"]));
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
    let ls = search::Search::new(root.clone(), search::Operation::Ls);
    let listed = call(
        &ls,
        json!({"path":"src","pattern":"nested/**/*.rs","kind":"file"}),
    )
    .await;
    let entries: Value = serde_json::from_str(&listed.content).unwrap();
    assert_eq!(
        entries["entries"].as_array().unwrap().len(),
        1,
        "{}",
        listed.content
    );
    assert_eq!(entries["entries"][0]["path"], "src/nested/deep/file.rs");

    let grep = search::Search::new(root, search::Operation::Grep);
    let matches = call(
        &grep,
        json!({"path":"src","glob":"nested/**/*.rs","pattern":"HELLO","output_mode":"files"}),
    )
    .await;
    let matches: Value = serde_json::from_str(&matches.content).unwrap();
    assert_eq!(matches["files"], json!(["src/nested/deep/file.rs"]));
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
    let receipt: Value = serde_json::from_str(&result.content).unwrap();
    assert!(!result.is_error);
    assert_eq!(receipt["changed_files"], 2);
    assert_eq!(receipt["files"].as_array().unwrap().len(), 2);
    assert_eq!(receipt["failures"].as_array().unwrap().len(), 1);
    assert_eq!(
        fs::read_to_string(dir.path("crlf.txt")).unwrap(),
        "new\r\nkeep\r\n"
    );
    assert_eq!(fs::read_to_string(dir.path("new.txt")).unwrap(), "hello\n");
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
async fn ls_global_topn_sort_and_unreadable_candidate() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("a")).unwrap();
    fs::write(dir.path("a/first.rs"), "FIRST").unwrap();
    fs::write(dir.path("z.rs"), "SECOND").unwrap();
    let ls = search::Search::new(dir.root(), search::Operation::Ls);
    let res = call(&ls, json!({"pattern":"**/*.rs","limit":1,"sort":"path"})).await;
    let parsed: Value = serde_json::from_str(&res.content).unwrap();
    assert_eq!(parsed["entries"][0]["path"], "a/first.rs");
    assert_eq!(parsed["truncated"], true);
    assert_eq!(parsed["scan_incomplete"], false);
    // Preserve precise nanosecond mtime sort, even for same-second writes.
    let newer = dir.path("z.rs");
    std::thread::sleep(std::time::Duration::from_millis(15));
    fs::write(&newer, "NEWEST").unwrap();
    let res = call(&ls, json!({"pattern":"**/*.rs","limit":1,"sort":"mtime"})).await;
    let parsed: Value = serde_json::from_str(&res.content).unwrap();
    assert_eq!(parsed["entries"][0]["path"], "z.rs");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let a = dir.path("a/first.rs");
        fs::set_permissions(&a, fs::Permissions::from_mode(0o0)).unwrap();
        let res = call(&ls, json!({"pattern":"**/*.rs","limit":2,"sort":"path"})).await;
        let parsed: Value = serde_json::from_str(&res.content).unwrap();
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 2);
        fs::set_permissions(&a, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
#[tokio::test]
async fn recursive_ls_can_ignore_git_and_hidden_files() {
    let dir = Tree::new();
    fs::create_dir_all(dir.path("target")).unwrap();
    fs::create_dir_all(dir.path("src")).unwrap();
    fs::write(dir.path(".gitignore"), "target/\n").unwrap();
    fs::write(dir.path("target/generated.rs"), "ignored\n").unwrap();
    fs::write(dir.path("src/lib.rs"), "useful\n").unwrap();
    fs::write(dir.path(".hidden.rs"), "hidden\n").unwrap();
    let ls = search::Search::new(dir.root(), search::Operation::Ls);
    let result = call(
        &ls,
        json!({"pattern":"**/*.rs","gitignore":true,
        "skip_hidden":true,"kind":"file"}),
    )
    .await;
    let data: Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(data["entries"].as_array().unwrap().len(), 1);
    assert_eq!(data["entries"][0]["path"], "src/lib.rs");
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
        "glob":"*.py","output_mode":"files"}),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&wrong_glob.content).unwrap()["files"],
        json!([])
    );
    let ignored = call(
        &grep,
        json!({"path":"code.rs","pattern":"NEEDLE",
        "gitignore":true,"output_mode":"files"}),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&ignored.content).unwrap()["files"],
        json!([])
    );
    let all = call(&grep, json!({"pattern":"NEEDLE","output_mode":"files"})).await;
    assert_eq!(
        serde_json::from_str::<Value>(&all.content).unwrap()["files"],
        json!(["code.rs"])
    );
    let reader = text::Text::new(root, text::Operation::Read);
    let invalid = call(&reader, json!({"path":"binary.txt"})).await;
    assert!(invalid.is_error);
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
    let before = fs::metadata(dir.path("file.txt")).unwrap();
    let again = call(&text, json!({"path":"file.txt","content":"same"})).await;
    let receipt: Value = serde_json::from_str(&again.content).unwrap();
    assert_eq!(receipt["changed"], false);
    assert_eq!(receipt["bytes_written"], 0);
    let after = fs::metadata(dir.path("file.txt")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(before.ino(), after.ino());
    }
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
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
    assert_eq!(
        serde_json::from_str::<Value>(&noop.content).unwrap()["changed"],
        false
    );
}
#[tokio::test]
async fn mcp_format_accepts_complete_file_only_and_preserves_mime() {
    let dir = Tree::new();
    fs::write(dir.path("small.png"), b"binary-payload").unwrap();
    let read = binary::Binary::new(dir.root(), binary::Operation::Read);
    let result = call(&read, json!({"path":"small.png","format":"mcp"})).await;
    assert!(!result.is_error);
    let value: Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(value["type"], "image");
    assert_eq!(value["mime_type"], "image/png");
    assert_eq!(value["transport"], "encoded_json");
    assert!(read
        .validate(&json!({"path":"small.png","format":"mcp","offset":1}))
        .is_err());
    let small = call(&read, json!({"path":"small.png","format":"mcp","limit":2})).await;
    assert!(small.is_error);
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
    let response = call(&grep, json!({"pattern":"missing","output_mode":"files"})).await;
    let parsed: Value = serde_json::from_str(&response.content).unwrap();
    assert_eq!(parsed["scan_incomplete"], true);
    assert_eq!(parsed["incomplete_reason"], "content_budget");
    assert!(response.truncated);
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
fn tool_schemas_cover_all_current_dotlink_parameters() {
    let tree = Tree::new();
    let root = tree.root();
    let surface: Vec<(ToolSpecVec, &[&str])> = vec![
        (
            ToolSpecVec::new(search::Search::new(root.clone(), search::Operation::Ls)),
            &[
                "path",
                "pattern",
                "depth",
                "kind",
                "sort",
                "limit",
                "gitignore",
                "skip_hidden",
                "parallel",
            ],
        ),
        (
            ToolSpecVec::new(search::Search::new(root.clone(), search::Operation::Grep)),
            &[
                "pattern",
                "path",
                "glob",
                "literal",
                "ignore_case",
                "output_mode",
                "before",
                "after",
                "numbered",
                "offset",
                "limit",
                "gitignore",
                "skip_hidden",
                "parallel",
            ],
        ),
        (
            ToolSpecVec::new(text::Text::new(root.clone(), text::Operation::Read)),
            &["path", "offset", "limit", "numbered", "parallel"],
        ),
        (
            ToolSpecVec::new(text::Text::new(root.clone(), text::Operation::Write)),
            &["path", "content", "create_only", "expected_sha256"],
        ),
        (
            ToolSpecVec::new(edit::Edit::new(root.clone())),
            &["path", "edits"],
        ),
        (
            ToolSpecVec::new(patch::Patch::new(root.clone())),
            &["dir", "patch", "best_effort"],
        ),
        (
            ToolSpecVec::new(binary::Binary::new(root.clone(), binary::Operation::Read)),
            &["path", "offset", "limit", "format", "parallel"],
        ),
        (
            ToolSpecVec::new(binary::Binary::new(root.clone(), binary::Operation::Write)),
            &["path", "data", "encoding", "create_only", "expected_sha256"],
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
        json!({"pattern":"hello","gitignore":true,"output_mode":"files"}),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&filtered.content).unwrap()["files"],
        json!([])
    );
    fs::write(dir.path(".gitignore"), "x".repeat(64 * 1024 + 1)).unwrap();
    let oversized = call(
        &grep,
        json!({"pattern":"hello","gitignore":true,"output_mode":"files"}),
    )
    .await;
    assert!(oversized.is_error);
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
        json!({"pattern":"hello","gitignore":true,"output_mode":"files"}),
    )
    .await;
    assert!(capped.is_error, "{}", capped.content);
    assert!(capped.content.contains("gitignore search budget"));
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
    assert!(!output.is_error, "{}", output.content);
    let receipt: Value = serde_json::from_str(&output.content).unwrap();
    assert_eq!(receipt["changed_files"], 1);
    assert_eq!(receipt["files"].as_array().unwrap().len(), 1);
    assert_eq!(receipt["files"][0]["path"], "good.txt");
    assert_eq!(receipt["failures"].as_array().unwrap().len(), 1);
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
    let ls = search::Search::new(root.clone(), search::Operation::Ls);
    let listing = call(&ls, json!({"depth":3,"pattern":"**/*.txt"})).await;
    let listed: Value = serde_json::from_str(&listing.content).unwrap();
    let grep = search::Search::new(root.clone(), search::Operation::Grep);
    let found = call(&grep, json!({"pattern":"needle","output_mode":"files"})).await;
    let result: Value = serde_json::from_str(&found.content).unwrap();
    fs::set_permissions(&hidden, fs::Permissions::from_mode(0o700)).unwrap();
    if blocked {
        assert_eq!(listed["scan_incomplete"], true, "{}", listing.content);
        assert_eq!(listed["incomplete_reason"], "traversal_error");
        assert_eq!(result["scan_incomplete"], true, "{}", found.content);
        assert_eq!(result["incomplete_reason"], "traversal_error");
    }
    let restricted = dir.path("restricted.txt");
    fs::write(&restricted, "needle\n").unwrap();
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o000)).unwrap();
    let unreadable = fs::File::open(&restricted).is_err();
    let found = call(&grep, json!({"pattern":"needle","output_mode":"files"})).await;
    let result: Value = serde_json::from_str(&found.content).unwrap();
    let content = call(&grep, json!({"pattern":"needle","output_mode":"content"})).await;
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o600)).unwrap();
    if unreadable {
        assert_eq!(result["scan_incomplete"], true, "{}", found.content);
        assert_eq!(result["incomplete_reason"], "unreadable_files");
        assert!(content.content.contains("unreadable entries"));
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
        json!({"pattern":"needle","gitignore":true,"output_mode":"files"}),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&filtered.content).unwrap()["files"],
        json!([])
    );
    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o000)).unwrap();
    let inaccessible = fs::File::open(&ignore).is_err();
    let files = call(
        &grep,
        json!({"pattern":"needle","gitignore":true,"output_mode":"files"}),
    )
    .await;
    let content = call(
        &grep,
        json!({"pattern":"needle","gitignore":true,"output_mode":"content"}),
    )
    .await;
    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o600)).unwrap();
    if inaccessible {
        let data: Value = serde_json::from_str(&files.content).unwrap();
        assert_eq!(data["scan_incomplete"], true, "{}", files.content);
        assert_eq!(data["incomplete_reason"], "ignore_rules");
        assert!(content.content.contains("ignore rules unavailable"));
    }
    fs::remove_file(ignore).unwrap();
    let absent = call(
        &grep,
        json!({"pattern":"needle","gitignore":true,"output_mode":"files"}),
    )
    .await;
    let data: Value = serde_json::from_str(&absent.content).unwrap();
    assert_eq!(data["files"], json!(["hit.txt"]));
    assert_eq!(data["scan_incomplete"], false);
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
    assert!(!one.is_error, "{}", one.content);
    assert!(one.content.contains("long.txt:1:"));
    assert!(
        one.content.contains("matching line omitted"),
        "{}",
        one.content
    );
    assert!(
        !one.content.contains("truncated at 0 hits"),
        "{}",
        one.content
    );

    let all = call(
        &grep,
        json!({"pattern":"needle","limit":200,"output_mode":"content"}),
    )
    .await;
    assert!(!all.is_error, "{}", all.content);
    assert_eq!(all.content.matches(":needle\n").count(), 63);
    assert!(
        all.content.contains("many.txt:60:needle"),
        "{}",
        all.content
    );
    assert!(
        all.content.contains("other.txt:3:needle"),
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
    let ls = search::Search::new(root.clone(), search::Operation::Ls);
    let listed = call(&ls, json!({"pattern":"**/*","kind":"file"})).await;
    let entries: Value = serde_json::from_str(&listed.content).unwrap();
    let paths = entries["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"ordinary.txt"));
    assert!(paths.contains(&".dotlink-notes.tmp"));
    assert!(!paths.iter().any(|s| s.contains("0123456789abcdef")));
    let grep = search::Search::new(root, search::Operation::Grep);
    let result = call(
        &grep,
        json!({"pattern":"STAGING_SECRET","output_mode":"files"}),
    )
    .await;
    let files: Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(files["files"], json!(["ordinary.txt"]));
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
        "limit":1,"output_mode":"content"}),
    )
    .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.truncated);
    assert!(out.content.contains("hits.txt:1:"), "{}", out.content);
    assert!(
        out.content.contains("matching line omitted"),
        "{}",
        out.content
    );
    assert!(
        out.content.contains("long source lines abbreviated"),
        "{}",
        out.content
    );
    assert!(
        out.content.contains("result page stopped after 1 hits"),
        "{}",
        out.content
    );
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
fn windows_native_shell_identity_is_powershell() {
    let tree = Tree::new();
    let shell = super::shell::PowerShell::new(
        tree.0.clone(),
        PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
    )
    .unwrap();
    assert_eq!(shell.spec().name, "powershell");
}
