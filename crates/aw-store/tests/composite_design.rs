//! AW-P6-10M DESIGN-PROOF ONLY. This module does not ship an operator composite
//! backup API, manifest parser, writer freeze, or restore command. It exercises
//! the actual existing SQLite/SecretStore primitives to pin prerequisites for
//! a future separately reviewed all-or-nothing operator-only format.
#![cfg(unix)]

use aw_agent_core::PrincipalId;
use aw_store::{
    provider_secret_ref, ActorQueueRequest, QueueTarget, SecretStore, SessionBinding, SessionStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "aw-p6-10m-design-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn original_binding() -> SessionBinding {
    SessionBinding {
        working_directory: PathBuf::from("/fixture"),
        tool_root: PathBuf::from("/fixture"),
        system_prompt: String::new(),
        profile_hint: "fixture".into(),
        model_hint: "m".into(),
        destination_fingerprint: "0".repeat(64),
    }
}

#[test]
fn aw_p6_10n_maintenance_lease_fences_every_existing_direct_secretstore_handle() {
    let temp = Temp::new();
    let store = SessionStore::open(&temp.0).unwrap();
    // Open a real direct public vault handle BEFORE acquiring any lease; a
    // late Stage/Bundle process must not evade the fence by caching a handle.
    let vault = store.local_secret_store().unwrap();
    let direct = aw_store::LocalSecretStore::open(&temp.0).unwrap();
    let existing = provider_secret_ref("design_store", "before", 1).unwrap();
    vault.put(&existing, b"before-maintenance").unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();

    let attempted = provider_secret_ref("design_store", "during", 1).unwrap();
    assert!(
        vault
            .put(&attempted, b"not-allowed-under-maintenance")
            .is_err(),
        "AW-P6-10N: even the Store-provided vault handle must fail during maintenance"
    );
    assert!(
        direct
            .put(&attempted, b"not-allowed-from-cached-handle")
            .is_err(),
        "AW-P6-10N: direct preexisting LocalSecretStore handles cannot evade the fence"
    );
    assert!(
        vault.delete(&existing).is_err(),
        "AW-P6-10N: an encrypted item cannot disappear while the original backup lease is active"
    );
    assert_eq!(
        direct.get(&existing).unwrap().as_slice(),
        b"before-maintenance",
        "read-only owner inspection remains permitted during snapshot capture"
    );
    assert!(
        !direct.exists(&attempted).unwrap(),
        "denied writes create no private file or immutable credential metadata"
    );

    store.release_local_maintenance_lease(&lease).unwrap();
    vault.put(&attempted, b"after-maintenance").unwrap();
    direct.delete(&existing).unwrap();
    assert!(!vault.exists(&existing).unwrap());
    assert_eq!(
        vault.get(&attempted).unwrap().as_slice(),
        b"after-maintenance"
    );
}

#[test]
fn aw_p6_10n_new_master_initialization_and_existing_writer_are_fenced() {
    use fs2::FileExt;

    // An original fresh persistent Store has no vault master until the first
    // owner-authorized LocalSecretStore::open. Even that initial key creation
    // cannot race an original maintenance capture.
    let temp = Temp::new();
    let owner = SessionStore::open(&temp.0).unwrap();
    let lease = owner
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(
        aw_store::LocalSecretStore::open(&temp.0).is_err(),
        "a master key cannot be minted while a snapshot cut is active"
    );
    assert!(!temp.0.join("secrets/master.key").exists());
    owner.release_local_maintenance_lease(&lease).unwrap();

    let vault = aw_store::LocalSecretStore::open(&temp.0).unwrap();
    let key = provider_secret_ref("design_store", "writer", 1).unwrap();
    vault.put(&key, b"original-unchanged-value").unwrap();
    let shared =
        aw_platform::open_private_existing(&temp.0.join("secrets/.mutation.lock"), true).unwrap();
    FileExt::try_lock_shared(&shared).unwrap();
    assert!(
        owner
            .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
            .is_err(),
        "an in-flight vault writer must make snapshot admission fail fast"
    );
    let rejected_snapshot = temp
        .0
        .join("not-created-during-active-vault-writer.sqlite3");
    assert!(
        owner.backup(&rejected_snapshot).is_err(),
        "the actual original WAL snapshot must deny admission under a vault writer"
    );
    assert!(
        !rejected_snapshot.exists(),
        "denied maintenance cannot publish even an empty private SQLite snapshot"
    );
    drop(shared);
    // A denied admission must never leave an actual SQL maintenance lease
    // behind or deadlock the next legitimate owner attempt.
    let lease = owner
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    let mut wrong = lease.clone();
    wrong.generation += 1;
    assert!(
        owner.release_local_maintenance_lease(&wrong).is_err(),
        "stale/copied lease cannot unlock another generation's vault barrier"
    );
    assert!(vault.delete(&key).is_err());
    owner.release_local_maintenance_lease(&lease).unwrap();
    vault.delete(&key).unwrap();
}

#[test]
fn aw_p6_10n_untrusted_vault_lock_symlinks_and_hardlinks_fail_closed() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let temp = Temp::new();
    let owner = SessionStore::open(&temp.0).unwrap();
    let vault_dir = temp.0.join("secrets");
    DirBuilder::new().mode(0o700).create(&vault_dir).unwrap();
    let lock_path = vault_dir.join(".mutation.lock");
    let existing = temp.0.join("another-private-owner-file");
    fs::write(&existing, b"NEVER_CHANGE_UNRELATED_PRIVATE_BYTES").unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o600)).unwrap();

    symlink(&existing, &lock_path).unwrap();
    assert!(
        owner
            .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
            .is_err(),
        "unsafe symlink must not become the original vault gate"
    );
    assert!(
        aw_store::LocalSecretStore::open(&temp.0).is_err(),
        "master initialization must reject a symlinked lockfile"
    );
    assert_eq!(
        fs::read(&existing).unwrap(),
        b"NEVER_CHANGE_UNRELATED_PRIVATE_BYTES"
    );
    assert!(lock_path
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_file(&lock_path).unwrap();

    fs::hard_link(&existing, &lock_path).unwrap();
    assert!(
        owner
            .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
            .is_err(),
        "a shared hardlink inode must not become the vault gate"
    );
    assert!(aw_store::LocalSecretStore::open(&temp.0).is_err());
    fs::remove_file(&lock_path).unwrap();
    // A failed unsafe-lock attempt must not have left a SQL maintenance lease.
    let lease = owner
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    owner.release_local_maintenance_lease(&lease).unwrap();
}

#[test]
fn aw_p6_10n_restore_generation_transition_releases_original_file_fence() {
    let temp = Temp::new();
    let owner = SessionStore::open(&temp.0).unwrap();
    let vault = owner.local_secret_store().unwrap();
    let before = provider_secret_ref("design_store", "before_restore", 1).unwrap();
    vault
        .put(&before, b"before-restore-existing-ciphertext")
        .unwrap();
    let snapshot = temp.0.join("restore-original-generation.sqlite3");
    owner.backup(&snapshot).unwrap();
    owner.restore_backup(&snapshot).unwrap();
    // Successful restore replaced the original SQL maintenance generation:
    // the old maintenance token MUST NOT leave the private OS vault gate
    // permanently held. All historical provider/Tool no-replay rules remain.
    let after = provider_secret_ref("design_store", "after_restore", 1).unwrap();
    vault.put(&after, b"post-restore-vault-mutation").unwrap();
    assert_eq!(
        vault.get(&after).unwrap().as_slice(),
        b"post-restore-vault-mutation"
    );
    let lease = owner
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(vault.delete(&after).is_err());
    owner.release_local_maintenance_lease(&lease).unwrap();
    vault.delete(&after).unwrap();
}

#[test]
fn aw_p6_10n_other_process_direct_vault_handles_cannot_bypass_owner_fence() {
    const CHILD_ROOT: &str = "AW_P6_10N_MUTATION_CHILD_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let direct = aw_store::LocalSecretStore::open(&root).unwrap();
        let existing = provider_secret_ref("design_store", "cross_process_original", 1).unwrap();
        let other = provider_secret_ref("design_store", "cross_process_new", 1).unwrap();
        assert_eq!(
            direct.get(&existing).unwrap().as_slice(),
            b"persisted-original"
        );
        assert!(direct.put(&other, b"cross-process-new").is_err());
        assert!(direct.delete(&existing).is_err());
        assert!(!direct.exists(&other).unwrap());
        return;
    }
    let temp = Temp::new();
    let owner = SessionStore::open(&temp.0).unwrap();
    let vault = owner.local_secret_store().unwrap();
    let existing = provider_secret_ref("design_store", "cross_process_original", 1).unwrap();
    vault.put(&existing, b"persisted-original").unwrap();
    let lease = owner
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("aw_p6_10n_other_process_direct_vault_handles_cannot_bypass_owner_fence")
        .arg("--nocapture")
        .env(CHILD_ROOT, &temp.0)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "native independent process failed its vault-denial checks: stdout={} stderr={}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    owner.release_local_maintenance_lease(&lease).unwrap();
    vault.delete(&existing).unwrap();
}

#[test]
fn aw_p6_10n_kernel_releases_exclusive_vault_fence_after_owner_process_exit() {
    const CHILD_ROOT: &str = "AW_P6_10N_EXIT_WITH_FENCE_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let owner = SessionStore::open(&root).unwrap();
        let vault = owner.local_secret_store().unwrap();
        let existing = provider_secret_ref("design_store", "crash_survivor", 1).unwrap();
        vault.put(&existing, b"pre-exit-persisted").unwrap();
        let _lease = owner
            .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
            .unwrap();
        // Simulate hard process termination: no Rust destructors, no explicit
        // SQL lease release. The OS must release the underlying file lock.
        std::process::exit(0);
    }
    let temp = Temp::new();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("aw_p6_10n_kernel_releases_exclusive_vault_fence_after_owner_process_exit")
        .arg("--nocapture")
        .env(CHILD_ROOT, &temp.0)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "isolated owner fixture could not acquire fenced maintenance: stderr={}",
        String::from_utf8_lossy(&child.stderr)
    );
    let original = provider_secret_ref("design_store", "crash_survivor", 1).unwrap();
    let resumed = aw_store::LocalSecretStore::open(&temp.0).unwrap();
    assert_eq!(
        resumed.get(&original).unwrap().as_slice(),
        b"pre-exit-persisted"
    );
    // The dead process no longer holds an OS File lock, independently of
    // whether the SQL coordination lease has already been reaped/reviewed.
    let fresh = provider_secret_ref("design_store", "post_exit", 1).unwrap();
    resumed.put(&fresh, b"owner-operator-restarted").unwrap();
    assert_eq!(
        resumed.get(&fresh).unwrap().as_slice(),
        b"owner-operator-restarted"
    );
}

// The following receipt format and verifier are TEST-ONLY prototypes to prove
// that even an operator-owned 0700 directory must be validated before any
// restore. They MUST NOT be advertised as a product-supported backup format.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignFileReceipt {
    path: String,
    byte_len: u64,
    sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignManifest {
    format: String,
    store_id: String,
    files: Vec<DesignFileReceipt>,
}

fn receipt(root: &Path, name: &str) -> DesignFileReceipt {
    let bytes = fs::read(root.join(name)).unwrap();
    DesignFileReceipt {
        path: name.into(),
        byte_len: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}
fn check_design_manifest(root: &Path, manifest: &DesignManifest) -> Result<(), &'static str> {
    if manifest.format != "aw-composite-DESIGN-NOT-PRODUCTION"
        || manifest.store_id.is_empty()
        || manifest.files.len() != 3
    {
        return Err("unsupported/incomplete test-only manifest");
    }
    let names = manifest
        .files
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if !names.contains("sessions.sqlite3") || !names.contains("secrets/master.key") {
        return Err("test-only manifest is missing SQLite or the original master key");
    }
    let items = names
        .iter()
        .filter(|name| name.starts_with("secrets/items/"))
        .collect::<Vec<_>>();
    if items.len() != 1 {
        return Err("test-only manifest requires exactly one bounded secret item");
    }
    let basename = items[0].strip_prefix("secrets/items/").unwrap();
    let digest = basename
        .strip_prefix("sec_")
        .and_then(|name| name.strip_suffix(".bin"))
        .ok_or("test-only item ref has wrong shape")?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("test-only item ref has wrong SHA shape");
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in &manifest.files {
        let path = Path::new(&entry.path);
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            || !seen.insert(entry.path.as_str())
            || entry.sha256.len() != 64
        {
            return Err("unsafe/duplicate path or digest");
        }
        let f = root.join(path);
        let metadata = fs::symlink_metadata(&f).map_err(|_| "missing snapshot file")?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            return Err("snapshot file has unsafe ownership/mode/link shape");
        }
        let bytes = fs::read(&f).map_err(|_| "unreadable snapshot file")?;
        if bytes.len() as u64 != entry.byte_len
            || format!("{:x}", Sha256::digest(&bytes)) != entry.sha256
        {
            return Err("snapshot file changed after receipt was recorded");
        }
    }
    // A digest of a file alone is not enough. Re-read the actual SQLite
    // image as read-only data and bind this TEST-ONLY manifest to its exact
    // original store identity, application/schema and integrity check.
    let sqlite = rusqlite::Connection::open_with_flags(
        root.join("sessions.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|_| "SQLite image cannot be opened")?;
    let store_id: String = sqlite
        .query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "SQLite store identity missing")?;
    let schema: i64 = sqlite
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| "SQLite schema missing")?;
    let app: i64 = sqlite
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(|_| "SQLite application identity missing")?;
    let check: String = sqlite
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|_| "SQLite snapshot integrity check failed")?;
    if store_id != manifest.store_id || schema != 33 || app != 0x41575333 || check != "ok" {
        return Err("SQLite snapshot identity/schema/content does not match receipt");
    }
    Ok(())
}

#[test]
fn aw_p6_10m_test_only_operator_manifest_rejects_tamper_before_cold_restore() {
    const BODY: &str = "AW_P6_10M_SYNTHETIC_ENCRYPTED_QUEUE_INPUT";
    let original = Temp::new();
    let store = SessionStore::open(&original.0).unwrap();
    let owner = PrincipalId::new("local").unwrap();
    let (session, branch) = store
        .create(&owner, "operator composite design", original_binding())
        .unwrap();
    let epoch = store
        .workspace_writer(&owner, None, &session.workspace_id)
        .unwrap()
        .writer_epoch;
    let fact = store
        .actor_queue_enqueue(
            &owner,
            ActorQueueRequest {
                request_id: "one_real_queued_input".into(),
                session_id: session.id,
                branch_id: branch.id,
                recipient_site: "local".into(),
                sender: "r9g1a1g1".into(),
                message_id: 1,
                writer_epoch: epoch,
                target: QueueTarget::Auto,
                priority: 0,
            },
            BODY,
        )
        .unwrap();

    let stage = Temp::new();
    store.backup(&stage.0.join("sessions.sqlite3")).unwrap();
    let sqlite = rusqlite::Connection::open(stage.0.join("sessions.sqlite3")).unwrap();
    let store_id: String = sqlite
        .query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let check: String = sqlite
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    let secrets = stage.0.join("secrets");
    DirBuilder::new().mode(0o700).create(&secrets).unwrap();
    DirBuilder::new()
        .mode(0o700)
        .create(secrets.join("items"))
        .unwrap();
    let item = format!("secrets/items/{}.bin", fact.payload_ref);
    fs::copy(
        original.0.join("secrets/master.key"),
        secrets.join("master.key"),
    )
    .unwrap();
    fs::copy(original.0.join(&item), stage.0.join(&item)).unwrap();

    let names = ["sessions.sqlite3", "secrets/master.key", &item];
    let manifest = DesignManifest {
        format: "aw-composite-DESIGN-NOT-PRODUCTION".into(),
        store_id,
        files: names.iter().map(|name| receipt(&stage.0, name)).collect(),
    };
    let encoded = serde_json::to_vec(&manifest).unwrap();
    let path = stage.0.join("manifest.json");
    let mut receipt_file = aw_platform::open_private_new(&path).unwrap();
    use std::io::Write;
    receipt_file.write_all(&encoded).unwrap();
    receipt_file.sync_all().unwrap();
    drop(receipt_file);
    aw_platform::sync_parent(&path).unwrap();
    let reread: DesignManifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    check_design_manifest(&stage.0, &reread).unwrap();
    let mut forged_identity = DesignManifest {
        store_id: "other_store_without_signed_authority".into(),
        ..DesignManifest {
            format: reread.format.clone(),
            store_id: reread.store_id.clone(),
            files: reread.files.clone(),
        }
    };
    assert!(check_design_manifest(&stage.0, &forged_identity).is_err());
    forged_identity.store_id = reread.store_id.clone();
    forged_identity.files.swap(0, 1);
    assert!(
        check_design_manifest(&stage.0, &forged_identity).is_ok(),
        "order of separately attested file entries is not a content-grant"
    );

    let path = stage.0.join(&item);
    let original_blob = fs::read(&path).unwrap();
    let mut tampered = original_blob.clone();
    let last = tampered.last_mut().unwrap();
    *last ^= 0x80;
    fs::write(&path, tampered).unwrap();
    assert!(check_design_manifest(&stage.0, &reread).is_err());
    fs::write(&path, &original_blob).unwrap();
    assert!(check_design_manifest(&stage.0, &reread).is_ok());
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(original.0.join(&item), &path).unwrap();
    assert!(check_design_manifest(&stage.0, &reread).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(&path, original_blob).unwrap();
    // A freshly recreated private file inherits the host's umask; never
    // assume the test runner uses 077. Restore the original required 0600
    // owner-only mode before expecting the strict verifier to accept it.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(check_design_manifest(&stage.0, &reread).is_ok());

    // Only after the TEST-ONLY verification do we manually assemble a cold
    // restore. No product bundle/restore API is exercised or claimed here.
    drop(store);
    let recovered = Temp::new();
    DirBuilder::new()
        .mode(0o700)
        .create(recovered.0.join("secrets"))
        .unwrap();
    DirBuilder::new()
        .mode(0o700)
        .create(recovered.0.join("secrets/items"))
        .unwrap();
    for name in names {
        fs::copy(stage.0.join(name), recovered.0.join(name)).unwrap();
    }
    let store = SessionStore::open(&recovered.0).unwrap();
    assert_eq!(
        store
            .local_secret_store()
            .unwrap()
            .get(&fact.payload_ref)
            .unwrap()
            .as_slice(),
        BODY.as_bytes()
    );
}
