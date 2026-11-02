//! Behavioral compatibility tests for the local dotlink-aligned tool surface.
//! No running model, credentials or MCP transport is needed.
use super::*;
use aw_agent_core::{
    AccountOwner, BillingOwner, CancellationToken, CredentialCustody, InvocationIdentity, Tool,
    ToolContext, ToolOutput,
};
use base64::Engine as _;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct DurableArtifactStore(Mutex<Vec<aw_tools::filesystem::private_artifact::Record>>);
impl aw_tools::filesystem::private_artifact::Store for DurableArtifactStore {
    fn load(
        &self,
    ) -> std::result::Result<
        Vec<aw_tools::filesystem::private_artifact::Record>,
        aw_tools::filesystem::private_artifact::Error,
    > {
        Ok(self.0.lock().unwrap().clone())
    }
    fn reserve(
        &self,
        record: &aw_tools::filesystem::private_artifact::Record,
    ) -> std::result::Result<(), aw_tools::filesystem::private_artifact::Error> {
        self.0.lock().unwrap().push(record.clone());
        Ok(())
    }
    fn retire(
        &self,
        record: &aw_tools::filesystem::private_artifact::Record,
    ) -> std::result::Result<(), aw_tools::filesystem::private_artifact::Error> {
        let mut records = self.0.lock().unwrap();
        if let Some(at) = records.iter().position(|current| current == record) {
            records.remove(at);
        }
        Ok(())
    }
}

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
    fn durable_root(&self) -> Arc<aw_platform::scoped_fs::Root> {
        let registry = aw_tools::filesystem::private_artifact::Registry::open(Arc::new(
            DurableArtifactStore::default(),
        ))
        .unwrap();
        Arc::new(
            aw_platform::scoped_fs::Root::open_with_private_artifacts(&self.0, registry).unwrap(),
        )
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
        call_id: "host-direct".into(),
        source: aw_agent_core::ToolSource::trusted_host(),
        effect: aw_agent_core::ToolEffect::Unknown,
        execution_site: "local".into(),
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
#[cfg(unix)]
#[tokio::test]
async fn edit_and_patch_reject_distinct_symlink_aliases_of_one_target_before_mutation() {
    use std::os::unix::fs::symlink;
    let dir = Tree::new();
    fs::write(dir.path("a.txt"), "one\ntwo\n").unwrap();
    symlink("a.txt", dir.path("alias.txt")).unwrap();
    let root = dir.root();

    let edit = edit::Edit::new(root.clone());
    let rejected = call(
        &edit,
        json!({"edits":[
            {"path":"a.txt","old_text":"one","new_text":"ONE"},
            {"path":"alias.txt","old_text":"two","new_text":"TWO"}
        ]}),
    )
    .await;
    assert!(rejected.is_error, "{}", rejected.content);
    assert_eq!(fs::read_to_string(dir.path("a.txt")).unwrap(), "one\ntwo\n");

    let patch = patch::Patch::new(root);
    let rejected = call(
        &patch,
        json!({"patch":"*** Begin Patch\n*** Update File: a.txt\n@@\n-one\n+ONE\n*** Update File: alias.txt\n@@\n-two\n+TWO\n*** End Patch\n"}),
    )
    .await;
    assert!(rejected.is_error, "{}", rejected.content);
    assert_eq!(fs::read_to_string(dir.path("a.txt")).unwrap(), "one\ntwo\n");
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
    let mut cursor = aw_tools::read::Cursor { line: 1, byte: 0 };
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
            let next = aw_tools::read::Cursor {
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
async fn scoped_writes_keep_existing_mode_and_follow_final_symlink_target() {
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
    let result = call(&writer, json!({"path":"link.txt","content":"through-link"})).await;
    assert_eq!(result.content, "ok");
    assert!(fs::symlink_metadata(dir.path("link.txt"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_to_string(dir.path("private.sh")).unwrap(),
        "through-link"
    );
    assert_eq!(
        fs::metadata(dir.path("private.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "resolved-target replacement keeps target mode"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn final_content_symlink_matrix_preserves_link_and_lifecycle_targets_link() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    fs::write(tree.path("real.txt"), "old\n").unwrap();
    symlink("real.txt", tree.path("alias.txt")).unwrap();
    let root = tree.durable_root();

    let read = text::Text::new(root.clone(), text::Operation::Read);
    let observed = call(&read, json!({"path":"alias.txt","numbered":false})).await;
    assert_eq!(observed.content, "old\n");

    let write = text::Text::new(root.clone(), text::Operation::Write);
    let written = call(&write, json!({"path":"alias.txt","content":"write\n"})).await;
    assert_eq!(written.content, "ok");
    assert!(fs::symlink_metadata(tree.path("alias.txt"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_to_string(tree.path("real.txt")).unwrap(),
        "write\n"
    );

    let edit = edit::Edit::new(root.clone());
    let edited = call(
        &edit,
        json!({"path":"alias.txt","edits":[["write","edit"]]}),
    )
    .await;
    assert_eq!(edited.content, "ok");
    assert_eq!(fs::read_to_string(tree.path("real.txt")).unwrap(), "edit\n");
    assert!(fs::symlink_metadata(tree.path("alias.txt"))
        .unwrap()
        .file_type()
        .is_symlink());

    let patch = patch::Patch::new(root.clone());
    let patched = call(
        &patch,
        json!({"patch":"*** Begin Patch\n*** Update File: alias.txt\n@@\n-edit\n+patch\n*** End Patch\n"}),
    )
    .await;
    assert_eq!(patched.content, "ok");
    assert_eq!(
        fs::read_to_string(tree.path("real.txt")).unwrap(),
        "patch\n"
    );
    assert!(fs::symlink_metadata(tree.path("alias.txt"))
        .unwrap()
        .file_type()
        .is_symlink());

    fs::write(tree.path("real.bin"), b"abc").unwrap();
    symlink("real.bin", tree.path("alias.bin")).unwrap();
    let bread = binary::Binary::new(root.clone(), binary::Operation::Read);
    let binary_read = call(
        &bread,
        json!({"path":"alias.bin","offset":0,"limit":3,"format":"hex"}),
    )
    .await;
    assert!(!binary_read.is_error, "{}", binary_read.content);
    assert!(binary_read.content.contains("616263"));

    let bwrite = binary::Binary::new(root.clone(), binary::Operation::Write);
    let binary_write = call(
        &bwrite,
        json!({"path":"alias.bin","data":"eHl6","encoding":"base64"}),
    )
    .await;
    assert_eq!(binary_write.content, "ok");
    assert_eq!(fs::read(tree.path("real.bin")).unwrap(), b"xyz");
    assert!(fs::symlink_metadata(tree.path("alias.bin"))
        .unwrap()
        .file_type()
        .is_symlink());

    let bedit = binary::Binary::new(root.clone(), binary::Operation::Edit);
    let binary_edit = call(
        &bedit,
        json!({"path":"alias.bin","edits":[{"offset":1,"length":1,"data":"UQ=="}]}),
    )
    .await;
    assert_eq!(binary_edit.content, "ok");
    assert_eq!(fs::read(tree.path("real.bin")).unwrap(), b"xQz");
    assert!(fs::symlink_metadata(tree.path("alias.bin"))
        .unwrap()
        .file_type()
        .is_symlink());

    let rm = lifecycle::Lifecycle::new(root, lifecycle::Operation::Remove);
    let removed = call(&rm, json!({"paths":["alias.txt"]})).await;
    assert_eq!(removed.content, "ok");
    assert!(!tree.path("alias.txt").exists());
    assert_eq!(
        fs::read_to_string(tree.path("real.txt")).unwrap(),
        "patch\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_discovery_root_keeps_lexical_model_namespace() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    fs::create_dir_all(tree.path("real/sub")).unwrap();
    fs::write(tree.path("real/sub/code.rs"), "needle\n").unwrap();
    symlink("real", tree.path("alias")).unwrap();
    symlink("real", tree.path(".hidden-root")).unwrap();
    let outside = Tree::new();
    fs::write(outside.path("outside.rs"), "needle\n").unwrap();
    symlink(&outside.0, tree.path("outside-dir")).unwrap();
    let root = tree.root();

    let find = search::Search::new(root.clone(), search::Operation::Find);
    let page = call(
        &find,
        json!({"path":"alias","glob":"*.rs","kind":"file","limit":20}),
    )
    .await;
    assert!(!page.is_error, "{}", page.content);
    assert!(
        page.content.contains("f  alias/sub/code.rs\n"),
        "{}",
        page.content
    );
    assert!(!page.content.contains("real/sub/code.rs"));

    let hidden = call(
        &find,
        json!({"path":".hidden-root","glob":"*.rs","kind":"file","limit":20}),
    )
    .await;
    assert!(!hidden.is_error, "{}", hidden.content);
    assert!(hidden.content.contains("f  .hidden-root/sub/code.rs\n"));

    let grep = search::Search::new(root.clone(), search::Operation::Grep);
    let hit = call(
        &grep,
        json!({"path":"alias","pattern":"needle","mode":"content"}),
    )
    .await;
    assert!(!hit.is_error, "{}", hit.content);
    assert!(
        hit.content.contains("alias/sub/code.rs:1:needle"),
        "{}",
        hit.content
    );
    assert!(!hit.content.contains("real/sub/code.rs"));

    let exact_link = tree.path("file-alias.rs");
    symlink("real/sub/code.rs", &exact_link).unwrap();
    let exact = call(
        &grep,
        json!({"path":"file-alias.rs","pattern":"needle","mode":"content"}),
    )
    .await;
    assert!(
        exact.content.contains("file-alias.rs:1:needle"),
        "{}",
        exact.content
    );

    let denied = call(
        &find,
        json!({"path":"outside-dir","kind":"file","limit":20}),
    )
    .await;
    assert!(denied.is_error);
}

#[cfg(unix)]
#[tokio::test]
async fn final_content_symlink_dangling_outside_and_intermediate_fail_closed() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    let outside = Tree::new();
    fs::write(outside.path("secret.txt"), "outside\n").unwrap();

    symlink("missing.txt", tree.path("dangling.txt")).unwrap();
    symlink(outside.path("secret.txt"), tree.path("outside.txt")).unwrap();
    fs::create_dir(tree.path("real-dir")).unwrap();
    fs::write(tree.path("real-dir/inside.txt"), "inside\n").unwrap();
    symlink("real-dir", tree.path("dir-link")).unwrap();

    let root = tree.root();
    let write = text::Text::new(root.clone(), text::Operation::Write);
    let dangling = call(
        &write,
        json!({"path":"dangling.txt","content":"must-not-create"}),
    )
    .await;
    assert!(dangling.is_error);
    assert!(!tree.path("missing.txt").exists());
    assert!(fs::symlink_metadata(tree.path("dangling.txt"))
        .unwrap()
        .file_type()
        .is_symlink());

    let outside_write = call(
        &write,
        json!({"path":"outside.txt","content":"must-not-escape"}),
    )
    .await;
    assert!(outside_write.is_error);
    assert_eq!(
        fs::read_to_string(outside.path("secret.txt")).unwrap(),
        "outside\n"
    );

    let read = text::Text::new(root, text::Operation::Read);
    let intermediate = call(
        &read,
        json!({"path":"dir-link/inside.txt","numbered":false}),
    )
    .await;
    assert!(
        intermediate.is_error,
        "intermediate symlinks remain no-follow"
    );
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
fn job_runtime_config_rejects_spool_that_cannot_retain_one_maximum_graph() {
    struct ProviderFixture;
    impl Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }
    let tree = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    let mut options = crate::LocalToolOptions::new(&tree.0, crate::ToolMode::ReadOnly);
    options.selected_tools = vec!["read".into(), "batch".into()];
    options.limits.result_bytes = 8192;
    // Enough for one native result reservation, but not for MAX_OPS retained
    // node results. A fresh valid Graph must never self-starve into `skip`.
    options.limits.jobs.max_spool_bytes = 8192;
    assert!(matches!(
        crate::configure(builder, actor, &options),
        Err(Error::Config(message)) if message.contains("maximum-size Graph")
    ));

    // The same tiny Job quota is irrelevant to a direct-only selection: no
    // Job runtime is advertised or constructed, so direct read remains valid.
    let actor = PrincipalId::new("local").unwrap();
    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    options.selected_tools = vec!["read".into()];
    assert!(crate::configure(builder, actor, &options).is_ok());
}

#[cfg(unix)]
#[test]
fn shell_graph_spool_preflight_counts_terminal_and_both_streams() {
    struct ProviderFixture;
    impl Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }
    let tree = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    let mut options = crate::LocalToolOptions::new(&tree.0, crate::ToolMode::Trusted);
    options.bash = Some(
        std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            })
            .expect("shell fixture needs bash"),
    );
    options.selected_tools = vec!["bash".into()];
    let shell_per_node =
        (crate::job_host::SHELL_RESULT_BYTES + 2 * options.limits.shell_stream_bytes) as u64;
    options.limits.jobs.max_spool_bytes = shell_per_node
        .checked_mul(aw_tools::filesystem::batch::MAX_OPS as u64)
        .unwrap()
        - 1;
    assert!(matches!(
        crate::configure(builder, actor, &options),
        Err(Error::Config(message)) if message.contains("maximum-size Graph")
    ));
}

#[cfg(unix)]
#[test]
fn empty_trusted_selection_includes_shell_in_graph_spool_preflight() {
    struct ProviderFixture;
    impl Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }
    let tree = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    let mut options = crate::LocalToolOptions::new(&tree.0, crate::ToolMode::Trusted);
    options.bash = Some(
        std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            })
            .expect("shell fixture needs bash"),
    );
    let shell_per_node =
        (crate::job_host::SHELL_RESULT_BYTES + 2 * options.limits.shell_stream_bytes) as u64;
    options.limits.jobs.max_spool_bytes = shell_per_node
        .checked_mul(aw_tools::filesystem::batch::MAX_OPS as u64)
        .unwrap()
        - 1;
    assert!(options.selected_tools.is_empty());
    assert!(matches!(
        crate::configure(builder, actor, &options),
        Err(Error::Config(message)) if message.contains("maximum-size Graph")
    ));
}

#[test]
fn serial_host_policy_configures_job_runtime_without_revalidation_drift() {
    struct ProviderFixture;
    impl Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }
    let tree = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    let mut options = LocalToolOptions::new(&tree.0, ToolMode::ReadOnly);
    options.concurrency = crate::LocalToolConcurrency {
        graph_width: 1,
        parallelism: aw_agent_core::ToolParallelism {
            max_parallel_reads: 1,
            allow_parallel_shell: false,
            max_parallel_shell: 1,
        },
    };
    let agent = configure(builder, actor, &options)
        .unwrap()
        .build()
        .unwrap();
    let specs = agent.tool_specs();
    assert!(specs.iter().any(|spec| spec.name == "batch"));
    let read = specs.iter().find(|spec| spec.name == "read").unwrap();
    assert!(
        read.parameters["properties"].get("parallel").is_some(),
        "parallel is a stable advisory schema field; the host scheduler may still serialize it"
    );
}

#[test]
fn configured_limits_narrow_advertised_schema_and_direct_validation() {
    let tree = Tree::new();
    let mut limits = crate::LocalToolLimits::default();
    limits.schema.text_write_bytes = 4;
    limits.schema.patch_bytes = 16;
    limits.schema.binary_data_bytes = 8;
    limits.schema.binary_edit_ranges = 1;
    limits.schema.read_line_limit = 2;
    limits.result_bytes = 8192;
    limits.shell_stream_bytes = 2048;
    limits.edit_text_bytes = 8;
    limits = limits.validate().unwrap();

    let write = text::Text::with_limits(tree.root(), text::Operation::Write, limits);
    assert_eq!(
        write.spec().parameters["properties"]["content"]["maxLength"],
        4
    );
    assert!(write
        .validate(&serde_json::json!({"path":"a.txt","content":"12345"}))
        .is_err());

    let read = text::Text::with_limits(tree.root(), text::Operation::Read, limits);
    assert_eq!(read.spec().parameters["properties"]["lines"]["maximum"], 2);
    assert!(read
        .validate(&serde_json::json!({"path":"a.txt","lines":3}))
        .is_err());

    let patch = patch::Patch::with_limits(tree.root(), limits);
    assert_eq!(
        patch.spec().parameters["properties"]["patch"]["maxLength"],
        16
    );
    assert!(patch
        .validate(&serde_json::json!({"patch":"x".repeat(17)}))
        .is_err());

    let binary = binary::Binary::with_limits(tree.root(), binary::Operation::Write, limits);
    assert_eq!(
        binary.spec().parameters["properties"]["data"]["maxLength"],
        8
    );
    assert!(binary
        .validate(&serde_json::json!({"path":"a.bin","data":"AAAAAAAAAAAA","encoding":"base64"}))
        .is_err());

    let edit = edit::Edit::with_limits(tree.root(), limits);
    assert!(edit
        .validate(&serde_json::json!({
            "path":"a.txt",
            "edits":[["12345","6789"]]
        }))
        .is_err());
}

#[test]
fn local_tool_concurrency_graph_width_is_narrowing_only() {
    assert!(crate::LocalToolConcurrency {
        graph_width: 8,
        ..Default::default()
    }
    .validate()
    .is_ok());
    assert!(crate::LocalToolConcurrency {
        graph_width: 9,
        ..Default::default()
    }
    .validate()
    .is_err());
}

#[test]
fn local_tool_limits_reject_caller_owned_schema_capability_flags() {
    let mut limits = crate::LocalToolLimits::default();
    limits.schema.parallel_reads = false;
    assert!(
        limits.validate().is_ok(),
        "parallel_reads is a static AW schema capability independent of live scheduler ceilings"
    );
    let mut limits = crate::LocalToolLimits::default();
    limits.schema.typed_media = true;
    assert!(limits.validate().is_err());
}

#[tokio::test]
async fn mcp_binary_read_is_complete_object_only_under_tight_result_budget() {
    let tree = Tree::new();
    fs::write(tree.path("blob.bin"), vec![0x5a; 7000]).unwrap();
    let limits = crate::LocalToolLimits {
        result_bytes: 8192,
        ..Default::default()
    }
    .validate()
    .unwrap();
    let output = binary::Binary::with_limits(tree.root(), binary::Operation::Read, limits)
        .execute(
            context(),
            &serde_json::json!({
                "path":"blob.bin","offset":0,"limit":7000,"format":"mcp"
            }),
        )
        .await
        .unwrap();
    assert!(output.is_error);
    assert!(output.content.contains("format=mcp complete object"));
    assert!(output.content.contains("base64/hex byte windows"));
}

#[tokio::test]
async fn narrowed_read_defaults_remain_valid_and_use_host_ceilings() {
    let tree = Tree::new();
    fs::write(tree.path("lines.txt"), "a\nb\nc\n").unwrap();
    fs::write(tree.path("blob.bin"), b"0123456789abcdef").unwrap();
    let mut limits = crate::LocalToolLimits::default();
    limits.schema.read_line_limit = 2;
    limits.schema.binary_encoded_read_bytes = 8;
    limits.result_bytes = 8192;
    let limits = limits.validate().unwrap();

    let read = text::Text::with_limits(tree.root(), text::Operation::Read, limits)
        .execute(
            context(),
            &serde_json::json!({"path":"lines.txt","numbered":false}),
        )
        .await
        .unwrap();
    assert!(!read.is_error, "{}", read.content);
    assert!(
        read.truncated,
        "two-line host default must page a three-line file"
    );
    let read_page: serde_json::Value = serde_json::from_str(&read.content).unwrap();
    assert_eq!(read_page["status"], "more");
    assert_eq!(read_page["next"]["line"], 3);

    let bread = binary::Binary::with_limits(tree.root(), binary::Operation::Read, limits)
        .execute(
            context(),
            &serde_json::json!({"path":"blob.bin","format":"base64"}),
        )
        .await
        .unwrap();
    assert!(!bread.is_error, "{}", bread.content);
    let page: serde_json::Value = serde_json::from_str(&bread.content).unwrap();
    assert_eq!(page["bytes"], 8);
    assert_eq!(page["next_offset"], 8);
    assert_eq!(page["status"], "more offset=8");
}

#[tokio::test]
async fn configured_result_ceiling_bounds_read_search_and_binary_results() {
    let tree = Tree::new();
    fs::write(tree.path("long.txt"), "x\n".repeat(5000)).unwrap();
    for i in 0..1000 {
        fs::write(tree.path(&format!("item-{i:03}.txt")), "x").unwrap();
    }
    fs::write(tree.path("blob.bin"), vec![0x5a; 12_000]).unwrap();
    let limits = crate::LocalToolLimits {
        result_bytes: 8192,
        ..Default::default()
    }
    .validate()
    .unwrap();

    let read = text::Text::with_limits(tree.root(), text::Operation::Read, limits)
        .execute(
            context(),
            &serde_json::json!({"path":"long.txt","numbered":false}),
        )
        .await
        .unwrap();
    assert!(
        read.content.len() <= limits.result_bytes,
        "{}",
        read.content.len()
    );
    assert!(read.truncated);

    let listing = search::Search::with_limits(tree.root(), search::Operation::Ls, limits)
        .execute(context(), &serde_json::json!({"path":".","limit":4096}))
        .await
        .unwrap();
    assert!(listing.content.len() <= limits.result_bytes);
    assert!(listing.truncated);

    let binary_tool = binary::Binary::with_limits(tree.root(), binary::Operation::Read, limits);
    let mut offset = 0u64;
    let mut reconstructed = Vec::new();
    let mut pages = 0usize;
    loop {
        let binary = binary_tool
            .execute(
                context(),
                &serde_json::json!({
                    "path":"blob.bin","offset":offset,"limit":4096,"format":"base64"
                }),
            )
            .await
            .unwrap();
        assert!(!binary.is_error, "{}", binary.content);
        assert!(binary.content.len() <= limits.result_bytes);
        let body: serde_json::Value = serde_json::from_str(&binary.content).unwrap();
        reconstructed.extend(
            base64::engine::general_purpose::STANDARD
                .decode(body["data"].as_str().unwrap())
                .unwrap(),
        );
        pages += 1;
        match body.get("next_offset").and_then(serde_json::Value::as_u64) {
            Some(next) => {
                assert!(next > offset, "binary continuation must advance");
                assert_eq!(body["status"], format!("more offset={next}"));
                offset = next;
            }
            None => break,
        }
    }
    assert!(pages > 1, "tight result ceiling must create binary pages");
    assert_eq!(reconstructed, vec![0x5a; 12_000]);
}

#[tokio::test]
async fn live_invocation_result_budget_preserves_direct_paging_cursors() {
    let tree = Tree::new();
    fs::write(tree.path("long.txt"), "line-value\n".repeat(2000)).unwrap();
    for i in 0..500 {
        fs::write(tree.path(&format!("item-{i:03}.txt")), b"x").unwrap();
    }
    fs::write(tree.path("blob.bin"), vec![0x5a; 4096]).unwrap();
    let limits = crate::LocalToolLimits::default();
    let mut ctx = context();
    ctx.result_budget_bytes = 1024;

    let read = text::Text::with_limits(tree.root(), text::Operation::Read, limits)
        .execute(
            ctx.clone(),
            &serde_json::json!({"path":"long.txt","numbered":false}),
        )
        .await
        .unwrap();
    assert!(read.content.len() <= ctx.result_budget_bytes);
    assert!(read.truncated);
    let read_page: serde_json::Value = serde_json::from_str(&read.content).unwrap();
    assert_eq!(read_page["status"], "more");
    assert!(read_page["next"]["line"]
        .as_u64()
        .is_some_and(|line| line > 1));

    let listing = search::Search::with_limits(tree.root(), search::Operation::Ls, limits)
        .execute(ctx.clone(), &serde_json::json!({"path":".","limit":4096}))
        .await
        .unwrap();
    assert!(listing.content.len() <= ctx.result_budget_bytes);
    assert!(listing.truncated);
    assert!(listing
        .content
        .lines()
        .any(|line| line.starts_with("more offset=")));

    let binary = binary::Binary::with_limits(tree.root(), binary::Operation::Read, limits)
        .execute(
            ctx,
            &serde_json::json!({
                "path":"blob.bin","offset":0,"limit":4096,"format":"base64"
            }),
        )
        .await
        .unwrap();
    assert!(binary.content.len() <= 1024);
    let page: serde_json::Value = serde_json::from_str(&binary.content).unwrap();
    let next = page["next_offset"]
        .as_u64()
        .expect("tight binary page must advance");
    assert!(next > 0 && next < 4096);
    assert_eq!(page["status"], format!("more offset={next}"));
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
fn selected_job_surface_without_explicit_producer_fails_closed() {
    struct ProviderFixture;
    impl aw_agent_core::Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut aw_agent_core::DeltaSink<'_>,
        ) -> aw_agent_core::CompletionFuture<'a> {
            Box::pin(async { Err(aw_agent_core::Error::Protocol("provider unused")) })
        }
    }
    let tree = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let provider = Arc::new(ProviderFixture);
    for selection in [
        vec!["batch".into()],
        vec!["job_start".into()],
        vec!["job_list".into()],
        vec!["job_list".into(), "read".into()],
    ] {
        let builder = Agent::builder(provider.clone(), AgentOptions::new("test", "model"));
        let mut options = LocalToolOptions::new(&tree.0, ToolMode::ReadOnly);
        options.selected_tools = selection;
        assert!(configure(builder, actor.clone(), &options).is_err());
    }

    let builder = Agent::builder(provider, AgentOptions::new("test", "model"));
    let mut options = LocalToolOptions::new(&tree.0, ToolMode::ReadOnly);
    options.selected_tools = vec!["batch".into(), "read".into()];
    assert!(configure(builder, actor, &options).is_ok());
}

#[cfg(unix)]
#[test]
fn selecting_shell_implicitly_includes_only_required_job_controls() {
    struct ProviderFixture;
    impl aw_agent_core::Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut aw_agent_core::DeltaSink<'_>,
        ) -> aw_agent_core::CompletionFuture<'a> {
            Box::pin(async { Err(aw_agent_core::Error::Protocol("provider unused")) })
        }
    }
    let Some(bash) = std::env::var_os("AW_BASH").map(PathBuf::from) else {
        return;
    };
    let temp = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = aw_agent_core::Agent::builder(
        Arc::new(ProviderFixture),
        aw_agent_core::AgentOptions::new("fixture", "model"),
    );
    let mut options = LocalToolOptions::new(&temp.0, ToolMode::Trusted);
    options.bash = Some(bash);
    options.selected_tools = vec!["bash".into()];
    let agent = configure(builder, actor, &options)
        .unwrap()
        .build()
        .unwrap();
    let names = agent
        .tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(names.contains("bash"));
    for name in [
        "job_list",
        "job_get",
        "job_output",
        "job_ack",
        "job_cancel",
        "job_restart",
    ] {
        assert!(names.contains(name), "missing required Job control {name}");
    }
    assert!(!names.contains("batch"));
    assert!(!names.contains("job_start"));
}

#[cfg(unix)]
#[test]
fn trusted_batch_schema_includes_only_prepared_bash_shellnode() {
    struct ProviderFixture;
    impl aw_agent_core::Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut aw_agent_core::DeltaSink<'_>,
        ) -> aw_agent_core::CompletionFuture<'a> {
            Box::pin(async { Err(aw_agent_core::Error::Protocol("provider unused")) })
        }
    }
    let Some(bash) = std::env::var_os("AW_BASH").map(PathBuf::from) else {
        return;
    };
    let temp = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = aw_agent_core::Agent::builder(
        Arc::new(ProviderFixture),
        aw_agent_core::AgentOptions::new("fixture", "model"),
    );
    let mut options = LocalToolOptions::new(&temp.0, ToolMode::Trusted);
    options.bash = Some(bash);
    let agent = configure(builder, actor, &options)
        .unwrap()
        .build()
        .unwrap();
    let specs = agent.tool_specs();
    for constructor in ["batch", "job_start"] {
        let spec = specs.iter().find(|spec| spec.name == constructor).unwrap();
        let names = spec.parameters["properties"]["ops"]["items"]["prefixItems"][0]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            names.contains("bash"),
            "{constructor} must expose prepared bash"
        );
        assert!(!names.contains("pwsh"));
        assert!(!names.contains("powershell"));
    }
}

#[test]
fn read_only_graph_schemas_do_not_advertise_mutation_nodes() {
    struct ProviderFixture;
    impl aw_agent_core::Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut aw_agent_core::DeltaSink<'_>,
        ) -> aw_agent_core::CompletionFuture<'a> {
            Box::pin(async { Err(aw_agent_core::Error::Protocol("provider unused")) })
        }
    }
    let temp = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = aw_agent_core::Agent::builder(
        Arc::new(ProviderFixture),
        aw_agent_core::AgentOptions::new("fixture", "model"),
    );
    let options = LocalToolOptions::new(&temp.0, ToolMode::ReadOnly);
    let agent = configure(builder, actor, &options)
        .unwrap()
        .build()
        .unwrap();
    let specs = agent.tool_specs();
    for constructor in ["batch", "job_start"] {
        let spec = specs.iter().find(|spec| spec.name == constructor).unwrap();
        let names = spec.parameters["properties"]["ops"]["items"]["prefixItems"][0]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        for name in ["ls", "find", "grep", "read"] {
            assert!(names.contains(name), "missing read node {name}");
        }
        assert!(
            !names.contains("bread"),
            "direct-only bread leaked into Graph schema"
        );
        for name in [
            "write", "edit", "patch", "cp", "mv", "rm", "bwrite", "bedit",
        ] {
            assert!(!names.contains(name), "read-only graph exposed {name}");
        }
    }
}

#[test]
fn canonical_job_surface_registers_job_start_after_shellnode_migration() {
    struct ProviderFixture;
    impl aw_agent_core::Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _: &'a [aw_agent_core::Message],
            _: &'a str,
            _: &'a mut aw_agent_core::DeltaSink<'_>,
        ) -> aw_agent_core::CompletionFuture<'a> {
            Box::pin(async { Err(aw_agent_core::Error::Protocol("provider unused")) })
        }
    }
    let temp = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = aw_agent_core::Agent::builder(
        Arc::new(ProviderFixture),
        aw_agent_core::AgentOptions::new("fixture", "model"),
    );
    let options = LocalToolOptions::new(&temp.0, ToolMode::ReadOnly);
    let agent = configure(builder, actor, &options)
        .unwrap()
        .build()
        .unwrap();
    let names = agent
        .tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect::<std::collections::BTreeSet<_>>();
    for name in [
        "batch",
        "job_start",
        "job_list",
        "job_get",
        "job_output",
        "job_ack",
        "job_cancel",
        "job_restart",
    ] {
        assert!(names.contains(name), "missing canonical Job tool {name}");
    }
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
async fn native_discovery_does_not_hide_unowned_private_looking_names() {
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
    assert!(listed
        .content
        .lines()
        .any(|line| line == "f  .dotlink-321-0123456789abcdef.tmp"));
    assert!(listed
        .content
        .lines()
        .any(|line| line == "f  .aw-stage-321-0123456789abcdef.tmp"));
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
async fn private_mutation_protection_uses_exact_identity_not_filename_shape() {
    let tree = Tree::new();
    let lookalike = ".aw-stage-5-0123456789abcdef.tmp";
    fs::write(tree.path(lookalike), "ordinary\n").unwrap();
    fs::write(tree.path("target.txt"), "old\n").unwrap();
    let root = tree.root();

    let read = text::Text::new(root.clone(), text::Operation::Read);
    let ordinary = call(&read, json!({"path":lookalike,"numbered":false})).await;
    assert_eq!(ordinary.content, "ordinary\n");

    let find = search::Search::new(root.clone(), search::Operation::Find);
    let listed = call(
        &find,
        json!({"path":".","kind":"file","hidden":true,"limit":100}),
    )
    .await;
    assert!(listed.content.contains(lookalike), "{}", listed.content);

    let _guard = root.lock_mutations(&["target.txt".into()]).unwrap();
    let staged = root
        .stage_replace("target.txt", b"new\n", &|| false)
        .unwrap();
    let records = root.private_artifacts().records().unwrap();
    assert_eq!(records.len(), 1);
    let private_path = records[0].name.clone();
    assert!(root.is_private_path(&private_path).unwrap());

    // Syntax validation no longer reserves a magic prefix, but execution must
    // reject the exact owned object by identity.
    let arguments = json!({"path":private_path,"numbered":false});
    assert!(read.validate(&arguments).is_ok());
    let protected = call(&read, arguments).await;
    assert!(
        protected.is_error,
        "actual private stage became model-readable"
    );

    let hidden = call(
        &find,
        json!({"path":".","kind":"file","hidden":true,"limit":100}),
    )
    .await;
    assert!(
        !hidden.content.contains(&private_path),
        "{}",
        hidden.content
    );
    assert!(hidden.content.contains(lookalike), "{}", hidden.content);

    staged.commit().unwrap();
    assert!(root.private_artifacts().records().unwrap().is_empty());
    assert_eq!(fs::read(tree.path("target.txt")).unwrap(), b"new\n");
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
    use aw_tools::filesystem::lifecycle::RemoveOutcome;
    let tree = Tree::new();
    fs::write(tree.path("a.txt"), "a").unwrap();
    fs::write(tree.path("b.txt"), "b").unwrap();
    let root = tree.durable_root();
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
async fn lifecycle_hardlinks_are_regular_files_but_alias_roots_fail_preflight() {
    let tree = Tree::new();
    fs::write(tree.path("original"), b"safe").unwrap();
    fs::hard_link(tree.path("original"), tree.path("alias")).unwrap();
    let root = tree.durable_root();

    let read = text::Text::new(root.clone(), text::Operation::Read);
    let observed = call(&read, json!({"path":"original","numbered":false})).await;
    assert!(!observed.is_error, "{}", observed.content);
    assert_eq!(observed.content, "safe");

    let tool = lifecycle::Lifecycle::new(root.clone(), lifecycle::Operation::Remove);
    let aliases = call(&tool, json!({"paths":["original","alias"]})).await;
    assert!(aliases.is_error);
    assert_eq!(fs::read(tree.path("original")).unwrap(), b"safe");
    assert_eq!(fs::read(tree.path("alias")).unwrap(), b"safe");

    let single = call(&tool, json!({"paths":["original"]})).await;
    assert_eq!(single.content, "ok");
    assert!(!tree.path("original").exists());
    assert_eq!(fs::read(tree.path("alias")).unwrap(), b"safe");
}

#[cfg(unix)]
#[tokio::test]
async fn atomic_replace_of_hardlink_breaks_only_named_link() {
    let tree = Tree::new();
    fs::write(tree.path("original"), b"old").unwrap();
    fs::hard_link(tree.path("original"), tree.path("alias")).unwrap();
    let write = text::Text::new(tree.root(), text::Operation::Write);
    let result = call(&write, json!({"path":"original","content":"new"})).await;
    assert_eq!(result.content, "ok");
    assert_eq!(fs::read(tree.path("original")).unwrap(), b"new");
    assert_eq!(fs::read(tree.path("alias")).unwrap(), b"old");
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
    let rm = lifecycle::Lifecycle::new(tree.durable_root(), lifecycle::Operation::Remove);
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
fn volatile_workspace_configuration_omits_rm_and_selected_rm_fails_closed() {
    struct ProviderFixture;
    impl Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }
    let tree = Tree::new();
    let actor = PrincipalId::new("local").unwrap();
    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    let options = LocalToolOptions::new(&tree.0, ToolMode::Workspace);
    let agent = configure(builder, actor.clone(), &options)
        .unwrap()
        .build()
        .unwrap();
    let specs = agent.tool_specs();
    assert!(!specs.iter().any(|spec| spec.name == "rm"));
    for constructor in ["batch", "job_start"] {
        let spec = specs.iter().find(|spec| spec.name == constructor).unwrap();
        let names = spec.parameters["properties"]["ops"]["items"]["prefixItems"][0]["enum"]
            .as_array()
            .unwrap();
        assert!(!names.iter().any(|name| name == "rm"));
    }

    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    let mut selected = LocalToolOptions::new(&tree.0, ToolMode::Workspace);
    selected.selected_tools = vec!["rm".into()];
    assert!(configure(builder, actor, &selected).is_err());
}

#[cfg(unix)]
#[test]
fn durable_private_state_enables_rm_for_direct_and_graph_surfaces() {
    struct ProviderFixture;
    impl Provider for ProviderFixture {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(Error::Protocol("provider unused")) })
        }
    }
    let tree = Tree::new();
    let state = Tree::new();
    let private = state.path("private");
    aw_platform::ensure_private_dir(&private).unwrap();
    let actor = PrincipalId::new("local").unwrap();
    let builder = Agent::builder(
        Arc::new(ProviderFixture),
        AgentOptions::new("fixture", "model"),
    );
    let mut options = LocalToolOptions::new(&tree.0, ToolMode::Workspace);
    options.private_state_dir = Some(private);
    let agent = configure(builder, actor, &options)
        .unwrap()
        .build()
        .unwrap();
    let specs = agent.tool_specs();
    assert!(specs.iter().any(|spec| spec.name == "rm"));
    for constructor in ["batch", "job_start"] {
        let spec = specs.iter().find(|spec| spec.name == constructor).unwrap();
        let names = spec.parameters["properties"]["ops"]["items"]["prefixItems"][0]["enum"]
            .as_array()
            .unwrap();
        assert!(names.iter().any(|name| name == "rm"));
    }
}

#[test]
fn read_write_full_modes_are_backwards_compatible_but_never_widen() {
    use aw_tools::filesystem::access::{Access, Action};
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
#[cfg(unix)]
#[tokio::test]
async fn unix_utf8_control_names_are_reversible_model_paths() {
    let tree = Tree::new();
    let newline = "line\nbreak.txt";
    let tabbed = "tab\tname.txt";
    fs::write(tree.path(newline), "newline\n").unwrap();
    fs::write(tree.path(tabbed), "tabbed\n").unwrap();
    let root = tree.durable_root();

    let find = search::Search::new(root.clone(), search::Operation::Find);
    let listed = call(&find, json!({"kind":"file","limit":20})).await;
    assert!(!listed.is_error, "{}", listed.content);
    assert!(
        listed.content.contains(r#""line\nbreak.txt""#),
        "{}",
        listed.content
    );
    assert!(
        listed.content.contains(r#""tab\tname.txt""#),
        "{}",
        listed.content
    );

    let read = text::Text::new(root.clone(), text::Operation::Read);
    let exact = call(&read, json!({"path":newline,"numbered":false})).await;
    assert_eq!(exact.content, "newline\n");

    let grep = search::Search::new(root.clone(), search::Operation::Grep);
    let hit = call(
        &grep,
        json!({"pattern":"newline","path":".","mode":"content"}),
    )
    .await;
    assert!(
        hit.content.contains(r#""line\nbreak.txt":1:newline"#),
        "{}",
        hit.content
    );

    let rm = lifecycle::Lifecycle::new(root, lifecycle::Operation::Remove);
    let removed = call(&rm, json!({"paths":[newline]})).await;
    assert_eq!(removed.content, "ok");
    assert!(!tree.path(newline).exists());
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
