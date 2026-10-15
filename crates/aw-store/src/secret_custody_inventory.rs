//! AW-P6-10O: bounded, read-only original owner maintenance inspection of
//! ACTIVE independent encrypted SecretStore custody. Not a composite backup,
//! serializer, provider authorization or model-facing Tool. Existing SQLite
//! v33 audit and version-CAS are the sole metadata authority; existing AEAD
//! SecretStore is the sole decryption/authentication engine.
use crate::{
    actor_queue::{ActorQueueFact, QueueState},
    db, extension_secret, now,
    secret_store::LocalSecretStore,
    LocalMaintenanceLease, SecretStore, SessionStore,
};
use aw_agent_core::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

// Explicitly bounded operator-only snapshot inspection, not an unbounded
// whole-Store history crawler. Excess metadata/files require operator review.
const MAX_CUSTODY_AUDIT: usize = 32_768;
const MAX_CUSTODY_ITEMS: usize = 32_768;
const MAX_CUSTODY_ACTIVE: usize = 32_768;

/// Counts only. Opaque refs, ciphertext, plaintext and hashes MUST NEVER be
/// returned through the operator status surface or serialized over RPC.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveSecretCustodyInventory {
    pub queue: usize,
    pub extension: usize,
    pub provider: usize,
    pub oidc: usize,
    pub oauth_client: usize,
    pub oauth_token: usize,
    pub active_unique: usize,
    /// Present private files not required by any CURRENT active SQLite fact.
    /// May include retired/history-only ciphertext and pre-audit orphans;
    /// neither classification authorizes deleting or publishing those bytes.
    pub unreferenced_items: usize,
}

fn audit_rows(c: &Connection, action: &str) -> Result<Vec<(String, String)>> {
    let mut q = db(c.prepare(
        "SELECT session_id,data_json FROM audit WHERE action=?1 ORDER BY seq LIMIT 32769",
    ))?;
    let rows = db(q.query_map([action], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }))?;
    let output = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if output.len() > MAX_CUSTODY_AUDIT {
        return Err(Error::Limit(
            "encrypted custody audit exceeds bounded operator inventory",
        ));
    }
    Ok(output)
}

fn active_queue_refs(c: &Connection) -> Result<Vec<(String, String)>> {
    let mut newest = BTreeMap::new();
    for (session, encoded) in audit_rows(c, "actor.queue")? {
        let fact: ActorQueueFact = crate::decode(&encoded)?;
        if fact.request.session_id != session {
            return Err(Error::Denied("queued Input audit Session identity changed"));
        }
        newest.insert((session, fact.request.request_id.clone()), fact);
    }
    let mut refs = Vec::new();
    for fact in newest.into_values() {
        if matches!(
            fact.state,
            QueueState::Pending | QueueState::Claimed | QueueState::Accepted
        ) {
            if fact.payload_sha256.len() != 64
                || !fact.payload_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::Denied("invalid original queued Input digest"));
            }
            refs.push((fact.payload_ref, fact.payload_sha256));
        }
    }
    Ok(refs)
}

// Every query includes CURRENT metadata row and corresponding original
// immutable version record, not merely a status string or an opaque ref.
// Missing/mismatched/superseded version rows are correctness failures,
// NEVER evidence that an active secret can be omitted from the backup.
fn current_versioned_refs(c: &Connection, sql: &str, class: &'static str) -> Result<Vec<String>> {
    let mut q = db(c.prepare(sql))?;
    let rows = db(q.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    }))?;
    let results = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if results.len() > MAX_CUSTODY_ACTIVE {
        return Err(Error::Limit(
            "versioned encrypted custody exceeds operator inventory cap",
        ));
    }
    let mut refs = Vec::with_capacity(results.len());
    for (selected, version_ref, version_state) in results {
        let valid = selected.as_ref().is_some_and(|value| {
            version_ref.as_ref() == Some(value) && version_state.as_deref() == Some("active")
        });
        if !valid {
            return Err(Error::Denied(match class {
                "provider" => "current Provider credential version is missing or not active",
                "oidc" => "current OIDC secret version is missing or not active",
                "oauth_client" => "current OAuth client-secret version is missing or not active",
                _ => "current OAuth token version is missing or not active",
            }));
        }
        refs.push(selected.expect("checked above"));
    }
    Ok(refs)
}

const PROVIDER_SQL: &str = "SELECT a.secret_ref,v.secret_ref,v.status FROM provider_accounts a
     LEFT JOIN provider_secret_versions v ON v.provider_account_id=a.provider_account_id
       AND v.version=a.secret_version
     WHERE a.status='active' AND a.credential_custody='server_secret' LIMIT 32769";
const OIDC_SQL: &str = "SELECT a.client_secret_ref,v.secret_ref,v.status FROM oidc_providers a
     LEFT JOIN oidc_provider_secret_versions v ON v.provider_id=a.provider_id
       AND v.version=a.client_secret_version
     WHERE a.status='active' AND a.client_auth_method<>'none' LIMIT 32769";
const OAUTH_CLIENT_SQL: &str =
    "SELECT a.client_secret_ref,v.secret_ref,v.status FROM oauth_connector_accounts a
     LEFT JOIN oauth_connector_client_secret_versions v ON v.connector_id=a.connector_id
       AND v.version=a.client_secret_version
     WHERE a.status IN ('active','configured') AND a.client_auth_method<>'none' LIMIT 32769";
const OAUTH_TOKEN_SQL: &str =
    "SELECT a.token_ref,v.token_ref,v.status FROM oauth_connector_accounts a
     LEFT JOIN oauth_connector_token_versions v ON v.connector_id=a.connector_id
       AND v.version=a.token_version
     WHERE a.status IN ('active','configured') AND a.token_ref IS NOT NULL LIMIT 32769";

fn exact_ref_name(value: &str) -> bool {
    value.len() == 68
        && value.starts_with("sec_")
        && value[4..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// AW-P6-10Q: the EXACT same original v33 active-reference / AEAD / Queue
/// digest verification, factored from the 10O lease-checked method. The caller
/// supplies a read-only SQLite connection and a private on-disk root. For
/// online owner Store access, only `verify_active_secret_custody` may expose
/// this computation after matching SQL/vault maintenance. The P/Q internal
/// independent snapshot checker uses it against its own staged files only;
/// no original Store, process, grants, network, new keys or mutable receipts.
pub(crate) fn verify_existing_secret_custody(
    c: &Connection,
    root: &Path,
) -> Result<ActiveSecretCustodyInventory> {
    let mut result = ActiveSecretCustodyInventory::default();
    let queued = active_queue_refs(c)?;
    let queued_digests: BTreeMap<String, String> = queued.iter().cloned().collect();
    let parts = [
        ("queue", queued.into_iter().map(|(r, _)| r).collect()),
        (
            "extension",
            extension_secret::active_refs_for_custody_inventory(&audit_rows(
                c,
                "extension.secret",
            )?)?,
        ),
        (
            "provider",
            current_versioned_refs(c, PROVIDER_SQL, "provider")?,
        ),
        ("oidc", current_versioned_refs(c, OIDC_SQL, "oidc")?),
        (
            "oauth_client",
            current_versioned_refs(c, OAUTH_CLIENT_SQL, "oauth_client")?,
        ),
        (
            "oauth_token",
            current_versioned_refs(c, OAUTH_TOKEN_SQL, "oauth_token")?,
        ),
    ];
    let mut selected = BTreeSet::new();
    for (kind, refs) in parts {
        match kind {
            "queue" => result.queue = refs.len(),
            "extension" => result.extension = refs.len(),
            "provider" => result.provider = refs.len(),
            "oidc" => result.oidc = refs.len(),
            "oauth_client" => result.oauth_client = refs.len(),
            _ => result.oauth_token = refs.len(),
        }
        for reference in refs {
            if !exact_ref_name(&reference) || !selected.insert(reference) {
                return Err(Error::Denied(
                    "active encrypted custody ref malformed or reused",
                ));
            }
            if selected.len() > MAX_CUSTODY_ACTIVE {
                return Err(Error::Limit(
                    "active encrypted custody exceeds operator limit",
                ));
            }
        }
    }
    result.active_unique = selected.len();

    let items = root.join("secrets/items");
    if !items.exists() && !selected.is_empty() {
        return Err(Error::Denied(
            "active encrypted custody has no private item directory",
        ));
    }
    if items.exists() {
        aw_platform::validate_private_dir(&items).map_err(Error::Io)?;
        let mut on_disk = BTreeSet::new();
        for entry in fs::read_dir(&items)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(Error::Denied(
                "encrypted custody has non-UTF8 item filename",
            ))?;
            // Temporary in-progress files may survive crashes; they are
            // quarantined for operator inspection, NEVER auto-deleted.
            let temporary = name.starts_with(".secret-")
                && name.ends_with(".tmp")
                && name.len() == ".secret-0000000000000000.tmp".len()
                && name.as_bytes()[8..24].iter().all(|b| b.is_ascii_hexdigit());
            let reference = name.strip_suffix(".bin");
            if !temporary && !reference.is_some_and(exact_ref_name) {
                return Err(Error::Denied(
                    "encrypted item has unsupported private filename",
                ));
            }
            // Original platform validation rejects symlinks, hardlinks,
            // other-owner files and all group/world readability.
            let private =
                aw_platform::open_private_existing(&entry.path(), false).map_err(Error::Io)?;
            // The original write-once vault permits at most 64KiB of
            // plaintext plus its 6-byte header, 12-byte nonce and 16-byte
            // AEAD tag. Orphan/temp items do not get unlimited file size.
            if private.metadata()?.len() > 64 * 1024 + 6 + 12 + 16 {
                return Err(Error::Limit("encrypted item exceeds original size bound"));
            }
            if !on_disk.insert(name.to_owned()) {
                return Err(Error::Denied("duplicate encrypted item filename"));
            }
            if on_disk.len() > MAX_CUSTODY_ITEMS {
                return Err(Error::Limit(
                    "encrypted item directory exceeds operator limit",
                ));
            }
            if temporary || !reference.is_some_and(|r| selected.contains(r)) {
                result.unreferenced_items += 1;
            }
        }
    }
    if selected.is_empty() {
        return Ok(result);
    }
    // Do NOT call SessionStore::local_secret_store() while holding the
    // original owner maintenance lock: on a missing key its first-use
    // path checks SQLite and might attempt key initialization. Instead
    // demand the existing master file, never mint a fresh key.
    let master = root.join("secrets/master.key");
    let _original = aw_platform::open_private_existing(&master, false).map_err(|_| {
        Error::Denied("required original SecretStore master key is missing or unsafe")
    })?;
    let vault = LocalSecretStore::open(root)?;
    for reference in &selected {
        if !vault.exists(reference)? {
            return Err(Error::Denied(
                "active encrypted SecretStore item is missing",
            ));
        }
        let original_bytes = vault.get(reference).map_err(|_| {
            Error::Denied("active encrypted SecretStore AEAD authentication failed")
        })?;
        if original_bytes.is_empty() {
            return Err(Error::Denied("active encrypted SecretStore item is empty"));
        }
        if let Some(expected_digest) = queued_digests.get(reference) {
            let digest = format!("{:x}", Sha256::digest(original_bytes.as_slice()));
            if &digest != expected_digest {
                return Err(Error::Denied(
                    "queued Input differs from original immutable digest",
                ));
            }
        }
        // Original Zeroizing buffer drops immediately; no result text or
        // digest is ever returned over a model/Host/RPC surface.
    }
    Ok(result)
}

impl SessionStore {
    /// Read-only verification of CURRENT active encrypted custody for one
    /// *already acquired* original operator-only `backup` maintenance lease.
    /// The original SQL maintenance lease and exclusive stable OS vault lock
    /// must both be held for the ENTIRE scan. A cloned or stale token cannot
    /// elevate a normal Session reader into a credential auditor.
    ///
    /// This checks original AEAD for every active ref, but never returns raw
    /// refs/plaintext/keys or removes orphan/retired ciphertext. It does not
    /// create a master key, publish a composite archive or approve replay.
    pub fn verify_active_secret_custody(
        &self,
        lease: &LocalMaintenanceLease,
    ) -> Result<ActiveSecretCustodyInventory> {
        if lease.purpose != "backup" {
            return Err(Error::Denied(
                "encrypted custody inventory requires original backup maintenance",
            ));
        }
        let root = self.directory().ok_or(Error::Denied(
            "encrypted custody inventory requires original persistent owner state",
        ))?;
        // Lock in the SAME order as original owner maintenance release:
        // file-fence slot, authoritative process, then native SQLite mutex.
        let fence = self.inner.vault_maintenance.lock().map_err(|_| {
            Error::Storage("owner vault maintenance fence poisoned; restart and inspect")
        })?;
        if !fence
            .as_ref()
            .is_some_and(|(original, _file)| original == lease)
        {
            return Err(Error::Denied(
                "encrypted custody inventory has no exact live owner vault fence",
            ));
        }
        let process = self.coordinated_process()?;
        let c = self.inner.conn.lock().map_err(|_| {
            Error::Storage("original SQLite owner mutex poisoned; restart and inspect")
        })?;
        let active: Option<i64> = db(c
            .query_row(
                "SELECT 1 FROM local_maintenance_lease
             WHERE singleton=1 AND state='active' AND purpose='backup'
               AND holder_process_id=?1 AND store_generation=?2
               AND generation=?3 AND expires_at=?4 AND expires_at>?5",
                rusqlite::params![
                    process.process_id,
                    process.store_generation,
                    lease.generation,
                    lease.expires_at,
                    now()
                ],
                |r| r.get(0),
            )
            .optional())?;
        if active != Some(1)
            || process.process_id != lease.holder_process_id
            || process.store_generation != lease.store_generation
        {
            return Err(Error::Denied(
                "encrypted custody inventory maintenance token expired or changed",
            ));
        }

        verify_existing_secret_custody(&c, root)
    }
}
