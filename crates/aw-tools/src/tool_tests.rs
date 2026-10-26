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
