//! AW-P6-10O: operator-only read-only encrypted SecretStore custody inventory.
//! These tests use real original SQLite v33, owner maintenance and the original
//! authenticated vault. All inputs are synthetic; no provider network/billing.
#![cfg(unix)]

use aw_agent_core::PrincipalId;
use aw_store::{
    provider_secret_ref, ActorQueueRequest, OAuthTokenSet, QueueTarget, SecretSettingScope,
    SecretStore, SessionBinding, SessionStore,
};
use std::{
    collections::BTreeSet,
    fs::{self, DirBuilder},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "aw-p6-10o-custody-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn binding() -> SessionBinding {
    SessionBinding {
        working_directory: PathBuf::from("/fixture"),
        tool_root: PathBuf::from("/fixture"),
        system_prompt: String::new(),
        profile_hint: "fixture".into(),
        model_hint: "fixture".into(),
        destination_fingerprint: "0".repeat(64),
    }
}
fn inventory(store: &SessionStore) -> aw_store::ActiveSecretCustodyInventory {
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    let result = store.verify_active_secret_custody(&lease);
    store.release_local_maintenance_lease(&lease).unwrap();
    result.unwrap()
}

#[test]
fn aw_p6_10o_pending_queue_custody_requires_exact_operator_lease_and_aead() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let actor = PrincipalId::new("local").unwrap();
    let (session, branch) = store
        .create(&actor, "active queued input", binding())
        .unwrap();
    let epoch = store
        .workspace_writer(&actor, None, &session.workspace_id)
        .unwrap()
        .writer_epoch;
    let fact = store
        .actor_queue_enqueue(
            &actor,
            ActorQueueRequest {
                request_id: "one_live_queue".into(),
                session_id: session.id.clone(),
                branch_id: branch.id.clone(),
                recipient_site: "local".into(),
                sender: "r9g1a1g1".into(),
                message_id: 1,
                writer_epoch: epoch,
                target: QueueTarget::Auto,
                priority: 0,
            },
            "SYNTHETIC_PRIVATE_QUEUED_TEXT",
        )
        .unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    let first = store.verify_active_secret_custody(&lease).unwrap();
    assert_eq!(first.queue, 1);
    assert_eq!(first.active_unique, 1);
    assert_eq!(first.unreferenced_items, 0);
    assert_eq!(
        first.extension + first.provider + first.oidc + first.oauth_client + first.oauth_token,
        0
    );
    let mut altered = lease.clone();
    altered.generation += 1;
    assert!(store.verify_active_secret_custody(&altered).is_err());
    store.release_local_maintenance_lease(&lease).unwrap();
    assert!(store.verify_active_secret_custody(&lease).is_err());

    // Removing a currently required blob must FAIL validation (not fabricate
    // an empty Input), while an arbitrary unused ciphertext is NOT promoted.
    let vault = store.local_secret_store().unwrap();
    vault.delete(&fact.payload_ref).unwrap();
    assert!(store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .and_then(|current| {
            let outcome = store.verify_active_secret_custody(&current);
            store.release_local_maintenance_lease(&current).unwrap();
            outcome
        })
        .is_err());
    vault
        .put(&fact.payload_ref, b"SYNTHETIC_PRIVATE_QUEUED_TEXT")
        .unwrap();
    assert_eq!(inventory(&store).queue, 1);
    // Even valid AEAD for a write-once opaque ref MUST match the immutable
    // original queued Input digest. A trusted local host able to delete and
    // re-encrypt *different* bytes under the same ref may not silently
    // convert the pending user's Input into a different authenticated body.
    vault.delete(&fact.payload_ref).unwrap();
    vault
        .put(&fact.payload_ref, b"DIFFERENT_BUT_VALID_AEAD_WITH_SAME_REF")
        .unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(
        store.verify_active_secret_custody(&lease).is_err(),
        "original immutable Queue payload digest must match decrypted AEAD body"
    );
    store.release_local_maintenance_lease(&lease).unwrap();
    vault.delete(&fact.payload_ref).unwrap();
    vault
        .put(&fact.payload_ref, b"SYNTHETIC_PRIVATE_QUEUED_TEXT")
        .unwrap();
    let unreferenced = provider_secret_ref("orphan-fixture", "unused", 1).unwrap();
    vault.put(&unreferenced, b"UNREFERENCED_ONLY").unwrap();
    let second = inventory(&store);
    assert_eq!(second.queue, 1);
    assert_eq!(second.unreferenced_items, 1);
    assert!(
        vault.exists(&unreferenced).unwrap(),
        "inventory must never erase unrelated ciphertext"
    );
}

#[test]
fn aw_p6_10o_active_extension_and_removed_history_are_distinct() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let actor = PrincipalId::new("local").unwrap();
    let (session, branch) = store
        .create(&actor, "extension-secret custody", binding())
        .unwrap();
    let epoch = store
        .workspace_writer(&actor, None, &session.workspace_id)
        .unwrap()
        .writer_epoch;
    let scope = SecretSettingScope {
        session_id: session.id.clone(),
        branch_id: branch.id.clone(),
        site: "local".into(),
        extension: "testext".into(),
        name: "token".into(),
        writer_epoch: epoch,
    };
    store
        .extension_secret_set(
            &actor,
            scope.clone(),
            None,
            128,
            b"SYNTHETIC_EXTENSION_SECRET",
        )
        .unwrap();
    assert_eq!(inventory(&store).extension, 1);
    store.extension_secret_reset(&actor, scope, 1).unwrap();
    let retired = inventory(&store);
    assert_eq!(retired.extension, 0);
    assert_eq!(retired.active_unique, 0);
}

#[test]
fn aw_p6_10o_retired_queued_ciphertext_and_unknown_items_are_not_minted_active() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let actor = PrincipalId::new("local").unwrap();
    let (session, branch) = store
        .create(&actor, "retired queued custody", binding())
        .unwrap();
    let epoch = store
        .workspace_writer(&actor, None, &session.workspace_id)
        .unwrap()
        .writer_epoch;
    let fact = store
        .actor_queue_enqueue(
            &actor,
            ActorQueueRequest {
                request_id: "retire_in_test".into(),
                session_id: session.id.clone(),
                branch_id: branch.id,
                recipient_site: "local".into(),
                sender: "r9g1a1g1".into(),
                message_id: 1,
                writer_epoch: epoch,
                target: QueueTarget::Auto,
                priority: 0,
            },
            "SYNTHETIC_RETIRED_QUEUED_PAYLOAD",
        )
        .unwrap();
    assert_eq!(inventory(&store).queue, 1);
    store
        .actor_queue_delete(&actor, &session.id, "retire_in_test", epoch)
        .unwrap();
    let vault = store.local_secret_store().unwrap();
    assert!(!vault.exists(&fact.payload_ref).unwrap());
    let retired = inventory(&store);
    assert_eq!(retired.queue, 0);
    assert_eq!(retired.active_unique, 0);
    // A formerly referenced encrypted item surviving unlink is not a
    // current queued Input and must not be "restored" as active authority.
    vault
        .put(&fact.payload_ref, b"SYNTHETIC_RETIRED_QUEUED_PAYLOAD")
        .unwrap();
    let orphan = inventory(&store);
    assert_eq!(orphan.queue, 0);
    assert_eq!(orphan.unreferenced_items, 1);
    assert!(vault.exists(&fact.payload_ref).unwrap());
}

#[test]
fn aw_p6_10o_empty_operator_inventory_cannot_mint_new_secret_master() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let empty = inventory(&store);
    assert_eq!(empty.active_unique, 0);
    assert_eq!(empty.unreferenced_items, 0);
    assert!(!root.0.join("secrets/master.key").exists());
}

#[test]
fn aw_p6_10o_provider_oidc_and_oauth_use_only_current_matching_version() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let actor = PrincipalId::new("unix:1000").unwrap();
    let vault = store.local_secret_store().unwrap();
    let models = BTreeSet::from(["fixture".into()]);
    store
        .create_server_provider_account(
            &actor,
            &vault,
            "current-provider",
            "openai",
            "https://fixture.invalid/v1",
            "team",
            "local",
            models,
            b"SYNTHETIC_PROVIDER_V1",
        )
        .unwrap();
    store
        .create_oidc_provider(
            &actor,
            &vault,
            "oidc-current",
            "https://login.example.invalid/tenant/",
            "client",
            "client_secret_post",
            BTreeSet::from(["openid".into()]),
            Some(b"SYNTHETIC_OIDC_V1"),
        )
        .unwrap();
    store
        .create_oauth_connector(
            &actor,
            &vault,
            "oauth-current",
            "https://auth.example.invalid/authorize",
            "https://auth.example.invalid/token",
            "client",
            "client_secret_post",
            BTreeSet::from(["read".into()]),
            Some(b"SYNTHETIC_OAUTH_CLIENT_V1"),
        )
        .unwrap();
    store
        .store_oauth_token_set(
            &actor,
            &vault,
            "oauth-current",
            &OAuthTokenSet {
                access_token: "SYNTHETIC_ACCESS".into(),
                refresh_token: Some("SYNTHETIC_REFRESH".into()),
                id_token: None,
                oidc_issuer: None,
                oidc_subject: None,
                token_type: "Bearer".into(),
                scopes: BTreeSet::from(["read".into()]),
                access_expires_at: None,
            },
        )
        .unwrap();
    let first = inventory(&store);
    assert_eq!(first.provider, 1);
    assert_eq!(first.oidc, 1);
    assert_eq!(first.oauth_client, 1);
    assert_eq!(first.oauth_token, 1);
    assert_eq!(first.active_unique, 4);
    // A configured connector with an already-persisted token reference is
    // inconsistent with ordinary current state, but the operator inventory
    // must still conservatively require/authenticate its original token.
    let metadata = rusqlite::Connection::open(root.0.join("sessions.sqlite3")).unwrap();
    metadata
        .execute(
            "UPDATE oauth_connector_accounts SET status='configured'
         WHERE connector_id='oauth-current'",
            [],
        )
        .unwrap();
    assert_eq!(inventory(&store).oauth_token, 1);
    metadata
        .execute(
            "UPDATE oauth_connector_accounts SET status='active'
         WHERE connector_id='oauth-current'",
            [],
        )
        .unwrap();

    store
        .rotate_server_provider_secret(&actor, &vault, "current-provider", b"SYNTHETIC_PROVIDER_V2")
        .unwrap();
    store
        .rotate_oidc_provider_secret(&actor, &vault, "oidc-current", b"SYNTHETIC_OIDC_V2")
        .unwrap();
    store
        .rotate_oauth_connector_client_secret(
            &actor,
            &vault,
            "oauth-current",
            b"SYNTHETIC_OAUTH_CLIENT_V2",
        )
        .unwrap();
    let rotated = inventory(&store);
    assert_eq!(
        rotated.provider + rotated.oidc + rotated.oauth_client + rotated.oauth_token,
        4
    );
    assert_eq!(rotated.active_unique, 4);

    store
        .revoke_oidc_provider(&actor, &vault, "oidc-current")
        .unwrap();
    store
        .revoke_oauth_connector(&actor, &vault, "oauth-current")
        .unwrap();
    store
        .revoke_server_provider_account(&actor, &vault, "current-provider")
        .unwrap();
    let retired = inventory(&store);
    assert_eq!(
        retired.active_unique, 0,
        "revoked credentials must not remain active backup obligations"
    );
}

#[test]
fn aw_p6_10o_active_provider_version_mismatch_rejected_before_any_model_calls() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let owner = PrincipalId::new("unix:1000").unwrap();
    let vault = store.local_secret_store().unwrap();
    store
        .create_server_provider_account(
            &owner,
            &vault,
            "version-corruption",
            "openai",
            "https://fixture.invalid/v1",
            "team",
            "local",
            BTreeSet::from(["fixture".into()]),
            b"SYNTHETIC_ACTIVE_VERSION",
        )
        .unwrap();
    assert_eq!(inventory(&store).provider, 1);
    // Inject an invalid version-status contradiction directly into the test's
    // own private v33 Store, without contacting or charging a model. The
    // operator must not mistake a filename for an active version receipt.
    let c = rusqlite::Connection::open(root.0.join("sessions.sqlite3")).unwrap();
    c.execute(
        "UPDATE provider_secret_versions SET status='superseded'
         WHERE provider_account_id='version-corruption' AND version=1",
        [],
    )
    .unwrap();
    assert!(
        store
            .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
            .and_then(|lease| {
                let checked = store.verify_active_secret_custody(&lease);
                store.release_local_maintenance_lease(&lease).unwrap();
                checked
            })
            .is_err(),
        "active account must match a current ACTIVE credential version"
    );
}

#[test]
fn aw_p6_10o_private_item_symlinks_hardlinks_and_group_readability_fail_closed() {
    use std::os::unix::fs::symlink;
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let vault = store.local_secret_store().unwrap();
    let original = provider_secret_ref("inventory", "real-private", 1).unwrap();
    vault
        .put(&original, b"REAL_CIPHERTEXT_NOT_FOR_PUBLIC")
        .unwrap();
    assert_eq!(inventory(&store).unreferenced_items, 1);
    let items = root.0.join("secrets/items");
    let other = provider_secret_ref("inventory", "untrusted-alias", 1).unwrap();
    let alias = items.join(format!("{other}.bin"));
    symlink(items.join(format!("{original}.bin")), &alias).unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(store.verify_active_secret_custody(&lease).is_err());
    store.release_local_maintenance_lease(&lease).unwrap();
    fs::remove_file(&alias).unwrap();

    fs::hard_link(items.join(format!("{original}.bin")), &alias).unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(store.verify_active_secret_custody(&lease).is_err());
    store.release_local_maintenance_lease(&lease).unwrap();
    fs::remove_file(&alias).unwrap();
    let file = items.join(format!("{original}.bin"));
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(store.verify_active_secret_custody(&lease).is_err());
    store.release_local_maintenance_lease(&lease).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(inventory(&store).unreferenced_items, 1);
    // Even an orphan/temp file cannot bypass the original 64KiB plus AEAD
    // envelope bound and force a later composite archive to be unbounded.
    let overflow = provider_secret_ref("inventory", "overflow", 1).unwrap();
    let too_large = items.join(format!("{overflow}.bin"));
    fs::write(&too_large, vec![0x41; 64 * 1024 + 6 + 12 + 17]).unwrap();
    fs::set_permissions(&too_large, fs::Permissions::from_mode(0o600)).unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(store.verify_active_secret_custody(&lease).is_err());
    store.release_local_maintenance_lease(&lease).unwrap();
    fs::remove_file(too_large).unwrap();
    assert_eq!(inventory(&store).unreferenced_items, 1);
}

#[test]
fn aw_p6_10o_custody_audit_metadata_limit_rejects_without_running_tools() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let actor = PrincipalId::new("local").unwrap();
    let (session, _) = store
        .create(&actor, "bounded owner metadata", binding())
        .unwrap();
    // Populate the existing immutable v33 audit through an independent
    // test-only SQL fixture, emulating a legitimate but over-budget old
    // history. No provider/Tool/process executor and no schema changes.
    let c = rusqlite::Connection::open(root.0.join("sessions.sqlite3")).unwrap();
    c.execute(
        "WITH RECURSIVE seq(n) AS (
           SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n<32769
         ) INSERT INTO audit(session_id,actor,action,data_json,created_at)
         SELECT ?1,'local','extension.secret','{}',0 FROM seq",
        [&session.id],
    )
    .unwrap();
    drop(c);
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(
        store.verify_active_secret_custody(&lease).is_err(),
        "operator inventory must reject more than 32768 original audit entries"
    );
    store.release_local_maintenance_lease(&lease).unwrap();
    assert!(
        !root.0.join("secrets/master.key").exists(),
        "bounded failed inspection never initializes an unrelated vault master"
    );
}

#[test]
fn aw_p6_10o_missing_or_tampered_current_provider_item_fails_closed_without_replay() {
    let root = Temp::new();
    let store = SessionStore::open(&root.0).unwrap();
    let actor = PrincipalId::new("unix:1000").unwrap();
    let vault = store.local_secret_store().unwrap();
    store
        .create_server_provider_account(
            &actor,
            &vault,
            "provider-loss",
            "openai",
            "https://fixture.invalid/v1",
            "team",
            "local",
            BTreeSet::from(["fixture".into()]),
            b"SYNTHETIC_KEY",
        )
        .unwrap();
    assert_eq!(inventory(&store).provider, 1);
    let source = rusqlite::Connection::open(root.0.join("sessions.sqlite3")).unwrap();
    let r: String = source
        .query_row(
            "SELECT secret_ref FROM provider_accounts WHERE provider_account_id='provider-loss'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let path = root.0.join("secrets/items").join(format!("{r}.bin"));
    let original = fs::read(&path).unwrap();
    vault.delete(&r).unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(store.verify_active_secret_custody(&lease).is_err());
    store.release_local_maintenance_lease(&lease).unwrap();
    // Restore exactly the original encrypted bytes without passing through a
    // provider/network executor; the test must not mint another credential.
    let mut file = aw_platform::open_private_new(&path).unwrap();
    use std::io::Write;
    file.write_all(&original).unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert_eq!(inventory(&store).provider, 1);
    let mut tampered = original;
    *tampered.last_mut().unwrap() ^= 0x80;
    fs::write(&path, tampered).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let lease = store
        .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
        .unwrap();
    assert!(store.verify_active_secret_custody(&lease).is_err());
    store.release_local_maintenance_lease(&lease).unwrap();
}
