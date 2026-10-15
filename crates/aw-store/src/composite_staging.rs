//! AW-P6-10P: bounded INTERNAL partial capture under the original Store owner
//! maintenance + SecretStore fence. No public backup, manifest, restore or
//! publication API. A partial stage is always deleted on owner scope exit;
//! only a future separately verified phase may authorize atomic publication.
#![cfg(unix)]
// This internal, non-public next-phase building block is intentionally unused
// by shipping code until atomic publication + offline restore are verified.
#![allow(dead_code)]

use crate::{db, secret_custody_inventory, SessionStore, StoreMode};
use aw_agent_core::{Error, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, DirBuilder, File},
    io::{Read, Write},
    os::unix::fs::DirBuilderExt,
    path::{Component, Path, PathBuf},
    time::Duration,
};

const CAPTURE_ID: &str = "aw-INTERNAL-PARTIAL-NOT-RECOVERABLE";
/// AW-P6-10R: stable on-disk complete-manifest V1 contract for operator-only
/// SAME-FILESYSTEM, no-overwrite publication. This is NOT a general user CLI,
/// remote/model grant or safe cold-restore implementation.
const COMPLETE_FORMAT: &str = "aw.operator-composite";
const COMPLETE_VERSION: u32 = 1;
/// AW-P6-10S: private inert marker required at every restored destination.
/// Normal SessionStore::open and trusted-local direct mode reject it BEFORE
/// registering an owner/Actor process or starting original recovery.
pub(crate) const COLD_QUARANTINE_MARKER: &str = ".aw-cold-restore-pending";
const MAX_BYTES: u64 = 512 * 1024 * 1024;
// Enough for all 32,768 bounded file receipts (~7MiB), not unbounded JSON.
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_PRIVATE_ITEMS: usize = 32_768;
const ITEM_LIMIT: u64 = 64 * 1024 + 6 + 12 + 16;
const SQLITE_SCHEMA: i64 = 33;
const SQLITE_APPLICATION: i64 = 0x41575333;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReceipt {
    path: String,
    length: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialManifest {
    format: String,
    /// Always false. The temporary artifact MUST NOT be used as a backup.
    complete: bool,
    store_id: String,
    application_id: i64,
    schema: i64,
    active_items: usize,
    unreferenced_items: usize,
    files: Vec<FileReceipt>,
}

/// On-disk schema V1, kept separate from the old Q PARTIAL manifest.
/// An unkeyed SHA-256 is a corruption check, NOT a signature or an encrypted
/// operator export. The exact source-independent SQLite/AEAD proof must pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteManifest {
    format: String,
    version: u32,
    complete: bool,
    store_id: String,
    /// Original SQLite restore-generation preserved as historical evidence;
    /// publishing NEVER mints a fresh WriterEpoch or process grant.
    restore_generation: u64,
    application_id: i64,
    schema: i64,
    active_items: usize,
    unreferenced_items: usize,
    files: Vec<FileReceipt>,
}
impl CompleteManifest {
    fn from_verified_partial(partial: PartialManifest, restore_generation: u64) -> Self {
        Self {
            format: COMPLETE_FORMAT.into(),
            version: COMPLETE_VERSION,
            complete: true,
            store_id: partial.store_id,
            restore_generation,
            application_id: partial.application_id,
            schema: partial.schema,
            active_items: partial.active_items,
            unreferenced_items: partial.unreferenced_items,
            files: partial.files,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublishStep {
    BeforeRename,
    AfterRenameBeforeParentSync,
}

/// Owns ONE new private `.partial` directory, never another operator's data.
/// Dropping it (even on error) removes its own transient files, not a usable
/// backup. External callers cannot obtain this private type or advertise it.
struct PartialCapture {
    directory: PathBuf,
    /// Disarm ONLY after the kernel reports atomic RENAME_NOREPLACE success.
    /// A later fsync failure leaves the named final artifact for inspection.
    published: bool,
}
impl PartialCapture {
    fn new(parent: &Path) -> Result<Self> {
        aw_platform::validate_private_dir(parent)?;
        let mut entropy = [0u8; 16];
        getrandom::fill(&mut entropy)
            .map_err(|_| Error::Storage("no OS entropy for partial capture"))?;
        let name = entropy
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let directory = parent.join(format!(".aw-internal-{name}.partial"));
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        builder.create(&directory)?; // exclusive: never adopt an existing dir/symlink
        aw_platform::validate_private_dir(&directory)?;
        Ok(Self {
            directory,
            published: false,
        })
    }
    fn verify(&self) -> Result<PartialManifest> {
        // Independent of any source Store: this is also called in a new OS
        // process after the original owner and vault no longer exist.
        verify_partial_directory(&self.directory)
    }

    /// AW-P6-10R: explicit ORIGINAL owner-only complete-manifest publication,
    /// still PRIVATE to the Store. No CLI, RPC, model Tool or restore entrypoint
    /// exists. Final V1 data is sensitive: SQLite plaintext + decryptable vault.
    /// A fault after rename returns Err but MUST NOT unlink the named artifact.
    #[cfg(target_os = "linux")]
    fn publish_complete<F>(
        mut self,
        owner: &SessionStore,
        final_name: &str,
        mut boundary: F,
    ) -> Result<PathBuf>
    where
        F: FnMut(PublishStep, &Path) -> Result<()>,
    {
        if owner.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "complete publication requires original exclusive Store owner",
            ));
        }
        let _process = owner.coordinated_process()?;
        // The publication is an explicit local owner action, not a copied
        // private stage that a *different* authenticated Store can promote.
        let partial = self.verify()?;
        let store_id: String = {
            let connection = owner
                .inner
                .conn
                .lock()
                .map_err(|_| Error::Storage("original owner SQLite lock poisoned"))?;
            db(connection.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |row| row.get(0),
            ))?
        };
        if store_id != partial.store_id {
            return Err(Error::Denied(
                "operator Store identity differs from captured original SQLite",
            ));
        }
        if !final_name.starts_with("aw-composite-")
            || final_name.len() > 96
            || !final_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error::Denied(
                "complete publication name must be a single bounded aw-composite-* basename",
            ));
        }
        let parent = self
            .directory
            .parent()
            .ok_or(Error::Denied("partial lacks private parent"))?;
        aw_platform::validate_private_dir(parent)?;
        let destination = parent.join(final_name);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(Error::Denied(
                "operator composite destination already exists",
            ));
        }
        // Build a DISTINCT stable complete V1 contract; never just toggle
        // the old partial marker and pretend the image is recoverable.
        let generation = inspect_restore_generation(&self.directory.join("sessions.sqlite3"))?;
        if generation != _process.store_generation {
            return Err(Error::Denied(
                "source owner generation differs from snapshot at publication",
            ));
        }
        let completed = CompleteManifest::from_verified_partial(partial, generation);
        let encoded = serde_json::to_vec(&completed)
            .map_err(|_| Error::Storage("failed to encode original complete manifest V1"))?;
        if encoded.len() > MAX_MANIFEST_BYTES {
            return Err(Error::Limit("complete V1 manifest exceeds 8MiB"));
        }
        let final_manifest = self.directory.join("manifest.json");
        let mut output = aw_platform::open_private_new(&final_manifest)?;
        output.write_all(&encoded)?;
        output.sync_all()?;
        drop(output);
        // Still `.partial` and NEVER publishable until the exact no-overwrite
        // kernel rename. Remove the old manifest only within our own Stage.
        fs::remove_file(self.directory.join("manifest.partial.json"))?;
        aw_platform::sync_parent(&final_manifest)?;
        verify_complete_payload(&self.directory)?;
        boundary(PublishStep::BeforeRename, &self.directory)?;
        // Same parent -> same filesystem. Linux renameat2(NO_REPLACE) makes
        // an existing empty directory, normal file or symlink a hard failure.
        // NO `std::fs::rename`/overwrite/check-then-rename TOCTOU fallback.
        rename_private_no_replace(&self.directory, &destination)?;
        // As soon as the kernel reports a successful rename, never unlink the
        // named artifact on subsequent failure, including parent-dir fsync.
        self.published = true;
        boundary(PublishStep::AfterRenameBeforeParentSync, &destination)?;
        aw_platform::sync_parent(&destination)?;
        verify_complete_directory(&destination)?;
        Ok(destination)
    }
}
/// AW-P6-10Q: read-only, source-independent verification of one owner-private
/// INCOMPLETE stage. No `SessionStore`, original owner maintenance token,
/// source vault, original process, Tool/Provider execution or public API is
/// required. A valid result means ONLY that the partial source is internally
/// self-consistent and authentic under its *staged* key; it is NOT a backup,
/// signed provenance or permission to activate/restore a runnable owner.
fn verify_partial_directory(directory: &Path) -> Result<PartialManifest> {
    aw_platform::validate_private_dir(directory)?;
    let source = directory.join("manifest.partial.json");
    let mut manifest_file = aw_platform::open_private_existing(&source, false)?;
    let mut encoded = Vec::new();
    (&mut manifest_file)
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut encoded)?;
    if encoded.len() > MAX_MANIFEST_BYTES {
        return Err(Error::Limit(
            "partial capture manifest exceeds bounded 8MiB",
        ));
    }
    let manifest: PartialManifest = serde_json::from_slice(&encoded)
        .map_err(|_| Error::Denied("invalid internal partial capture manifest"))?;
    if manifest.format != CAPTURE_ID
        || manifest.complete
        || manifest.schema != SQLITE_SCHEMA
        || manifest.application_id != SQLITE_APPLICATION
        || manifest.store_id.is_empty()
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_PRIVATE_ITEMS + 2
    {
        return Err(Error::Denied(
            "internal partial capture cannot be treated as completed",
        ));
    }
    verify_manifest_payload(
        directory,
        "manifest.partial.json",
        &manifest.store_id,
        manifest.application_id,
        manifest.schema,
        manifest.active_items,
        manifest.unreferenced_items,
        &manifest.files,
    )?;
    Ok(manifest)
}

fn require_exact_entries(root: &Path, expected: &BTreeSet<String>) -> Result<()> {
    aw_platform::validate_private_dir(root)?;
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let text = entry.file_name();
        let name = text
            .to_str()
            .ok_or(Error::Denied("non-UTF8 staged private entry"))?;
        if !actual.insert(name.to_owned()) {
            return Err(Error::Denied("duplicate staged private entry"));
        }
        if actual.len() > MAX_PRIVATE_ITEMS + 4 {
            return Err(Error::Limit("partial directory has too many entries"));
        }
    }
    if actual != *expected {
        return Err(Error::Denied(
            "partial directory contains missing or unreceipted entries",
        ));
    }
    Ok(())
}

fn valid_original_item_name(path: &str) -> bool {
    let Some(name) = path.strip_prefix("secrets/items/") else {
        return false;
    };
    let regular = name.strip_suffix(".bin").is_some_and(|stem| {
        stem.starts_with("sec_")
            && stem.len() == 68
            && stem[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    let temporary = name.starts_with(".secret-")
        && name.ends_with(".tmp")
        && name.len() == ".secret-0000000000000000.tmp".len()
        && name.as_bytes()[8..24]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit());
    regular || temporary
}

/// Source-independent verification of a PUBLISHED V1 complete archive.
/// The final directory name must not be an internal `.partial`; after crash
/// prior to rename, no complete marker inside a `.partial` can certify it.
fn verify_complete_directory(directory: &Path) -> Result<CompleteManifest> {
    if directory
        .file_name()
        .is_none_or(|name| name.to_string_lossy().ends_with(".partial"))
    {
        return Err(Error::Denied(
            "an incomplete partial is never a published backup",
        ));
    }
    verify_complete_payload(directory)
}

/// Same complete-format proof, usable INSIDE `.partial` ONLY immediately
/// before a mandatory atomic kernel RENAME_NOREPLACE publication boundary.
fn verify_complete_payload(directory: &Path) -> Result<CompleteManifest> {
    aw_platform::validate_private_dir(directory)?;
    let mut reader = aw_platform::open_private_existing(&directory.join("manifest.json"), false)?;
    let mut encoded = Vec::new();
    (&mut reader)
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut encoded)?;
    if encoded.len() > MAX_MANIFEST_BYTES {
        return Err(Error::Limit("complete V1 manifest exceeds bounded 8MiB"));
    }
    let manifest: CompleteManifest = serde_json::from_slice(&encoded)
        .map_err(|_| Error::Denied("invalid operator-composite V1 manifest"))?;
    if manifest.format != COMPLETE_FORMAT
        || manifest.version != COMPLETE_VERSION
        || !manifest.complete
        || manifest.schema != SQLITE_SCHEMA
        || manifest.application_id != SQLITE_APPLICATION
        || manifest.store_id.is_empty()
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_PRIVATE_ITEMS + 2
    {
        return Err(Error::Denied(
            "unsupported or incomplete operator-composite V1 manifest",
        ));
    }
    verify_manifest_payload(
        directory,
        "manifest.json",
        &manifest.store_id,
        manifest.application_id,
        manifest.schema,
        manifest.active_items,
        manifest.unreferenced_items,
        &manifest.files,
    )?;
    if manifest.restore_generation == 0
        || manifest.restore_generation
            != inspect_restore_generation(&directory.join("sessions.sqlite3"))?
    {
        return Err(Error::Denied(
            "original restore generation differs from completed V1 receipt",
        ));
    }
    Ok(manifest)
}

/// ONE Q/R underlying verifier for bounded original file receipt shape,
/// SQLite identity/quick_check, and ORIGINAL staged AEAD/Queue digest custody.
/// Never open this directory as a runnable SessionStore or initiate effects.
fn verify_manifest_payload(
    directory: &Path,
    manifest_name: &str,
    store_id: &str,
    application_id: i64,
    schema: i64,
    active_items: usize,
    unreferenced_items: usize,
    files: &[FileReceipt],
) -> Result<()> {
    let mut unique = BTreeSet::new();
    let mut total = 0u64;
    for entry in files {
        let path = Path::new(&entry.path);
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            || (entry.path.starts_with("secrets/items/") && path.components().count() != 3)
            || entry.sha256.len() != 64
            || !entry.sha256.bytes().all(|digit| digit.is_ascii_hexdigit())
            || !unique.insert(entry.path.clone())
            || !(entry.path == "sessions.sqlite3"
                || entry.path == "secrets/master.key"
                || entry.path.starts_with("secrets/items/"))
        {
            return Err(Error::Denied("unsafe or duplicate partial snapshot path"));
        }
        if entry.length > MAX_BYTES
            || (entry.path == "secrets/master.key" && entry.length != 32)
            || (entry.path.starts_with("secrets/items/") && entry.length > ITEM_LIMIT)
            || (entry.path.starts_with("secrets/items/") && !valid_original_item_name(&entry.path))
        {
            return Err(Error::Limit(
                "partial capture entry violates original bounds",
            ));
        }
        let file = aw_platform::open_private_existing(&directory.join(path), false)?;
        if file.metadata()?.len() != entry.length {
            return Err(Error::Denied(
                "staged file size differs from original manifest",
            ));
        }
        let mut hasher = Sha256::new();
        let mut copy = file;
        let length = hash_limited(&mut copy, &mut hasher, entry.length)?;
        if length != entry.length || format!("{:x}", hasher.finalize()) != entry.sha256 {
            return Err(Error::Denied(
                "staged private file does not match partial receipt",
            ));
        }
        total = total
            .checked_add(length)
            .ok_or(Error::Limit("capture byte overflow"))?;
        if total > MAX_BYTES {
            return Err(Error::Limit("partial capture exceeds size budget"));
        }
    }
    if !unique.contains("sessions.sqlite3") {
        return Err(Error::Denied("partial capture missing SQLite source"));
    }
    let expected_items = active_items
        .checked_add(unreferenced_items)
        .ok_or(Error::Limit("staged encrypted item count overflow"))?;
    let recorded_items = files
        .iter()
        .filter(|f| f.path.starts_with("secrets/items/"))
        .count();
    if recorded_items != expected_items
        || (expected_items != 0 && !unique.contains("secrets/master.key"))
    {
        return Err(Error::Denied(
            "partial capture file inventory does not match original custody summary",
        ));
    }
    let staged_items = directory.join("secrets/items");
    if staged_items.exists() {
        aw_platform::validate_private_dir(&staged_items)?;
        let actual_items = fs::read_dir(&staged_items)?
            .map(|entry| {
                entry.map(|value| format!("secrets/items/{}", value.file_name().to_string_lossy()))
            })
            .collect::<std::io::Result<BTreeSet<_>>>()?;
        let recorded_paths = unique
            .iter()
            .filter(|name| name.starts_with("secrets/items/"))
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual_items != recorded_paths {
            return Err(Error::Denied(
                "partial capture has omitted or extra private item files",
            ));
        }
    } else if expected_items != 0 {
        return Err(Error::Denied(
            "partial capture has no required item directory",
        ));
    }
    // Enumerate EACH staging directory exactly. A matching manifest checksum
    // must never excuse an omitted, injected or unsupported private file,
    // and a symlinked directory must never be followed during enumeration.
    let mut root_expected =
        BTreeSet::from([manifest_name.to_owned(), "sessions.sqlite3".to_owned()]);
    if directory.join("secrets").exists() {
        root_expected.insert("secrets".into());
        let secrets = directory.join("secrets");
        aw_platform::validate_private_dir(&secrets)?;
        let mut secrets_expected = BTreeSet::new();
        if unique.contains("secrets/master.key") {
            secrets_expected.insert("master.key".into());
        }
        if staged_items.exists() {
            secrets_expected.insert("items".into());
        }
        require_exact_entries(&secrets, &secrets_expected)?;
    }
    require_exact_entries(directory, &root_expected)?;
    let (actual_id, actual_application, actual_schema) =
        inspect_sqlite(&directory.join("sessions.sqlite3"))?;
    if actual_id != store_id || actual_application != application_id || actual_schema != schema {
        return Err(Error::Denied("captured original SQLite identity changed"));
    }
    // AW-P6-10Q: the decisive independent boundary. Re-open ONLY the STAGED
    // read-only SQLite v33 image and the STAGED, owner-private SecretStore.
    // Reuse the SAME O active-reference/version/AEAD/Queue-digest validator
    // as the live maintenance-gated Store; do not trust manifest hashes alone.
    // This does NOT create a new key or run the original Agent/Queue/Tool.
    let image = db(Connection::open_with_flags(
        directory.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ))?;
    let actual = secret_custody_inventory::verify_existing_secret_custody(&image, directory)?;
    if actual.active_unique != active_items || actual.unreferenced_items != unreferenced_items {
        return Err(Error::Denied(
            "staged active encrypted custody differs from original receipt counts",
        ));
    }
    Ok(())
}

/// Linux native same-parent, atomic and NO-OVERWRITE directory publication.
/// The file descriptor/paths are deliberately PRIVATE to the original Store
/// module. Other native platforms must separately implement and validate their
/// own original filesystem semantics before publication is enabled there.
#[cfg(target_os = "linux")]
fn rename_private_no_replace(from: &Path, to: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renameat2(
            oldfd: i32,
            old: *const std::ffi::c_char,
            newfd: i32,
            new: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    const AT_FDCWD: i32 = -100;
    const RENAME_NOREPLACE: u32 = 1;
    let old = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| Error::Denied("internal partial path contains NUL"))?;
    let new = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| Error::Denied("composite publication destination contains NUL"))?;
    // SAFETY: Linux renameat2(2) accepts these NUL-terminated valid buffers;
    // AT_FDCWD refers to current process, RENAME_NOREPLACE is documented.
    // Both paths share the same parent and the caller holds source ownership.
    let outcome = unsafe {
        renameat2(
            AT_FDCWD,
            old.as_ptr(),
            AT_FDCWD,
            new.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if outcome != 0 {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

impl Drop for PartialCapture {
    fn drop(&mut self) {
        // Do not follow another path/symlink. A malicious same-UID process is
        // outside the original local owner-lock threat model. Failed removal
        // is a quarantined `.partial` requiring operator inspection, NEVER an
        // automatically publishable backup.
        if !self.published && aw_platform::validate_private_dir(&self.directory).is_ok() {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

fn hash_limited(reader: &mut File, digest: &mut Sha256, limit: u64) -> Result<u64> {
    let mut count = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or(Error::Limit("capture byte overflow"))?;
        if count > limit {
            return Err(Error::Limit("private capture input exceeds exact bound"));
        }
        digest.update(&buffer[..read]);
    }
    Ok(count)
}

fn copy_private(source: &Path, target: &Path, limit: u64, name: &str) -> Result<FileReceipt> {
    let mut input = aw_platform::open_private_existing(source, false)?;
    let expected = input.metadata()?.len();
    if expected > limit {
        return Err(Error::Limit(
            "original private source exceeds capture budget",
        ));
    }
    let mut output = aw_platform::open_private_new(target)?;
    let mut hasher = Sha256::new();
    let mut count = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buf)?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or(Error::Limit("capture byte overflow"))?;
        if count > expected || count > limit {
            return Err(Error::Limit("private capture source changed"));
        }
        hasher.update(&buf[..read]);
        output.write_all(&buf[..read])?;
    }
    if count != expected {
        return Err(Error::Denied("private source changed while staged"));
    }
    output.sync_all()?;
    Ok(FileReceipt {
        path: name.into(),
        length: count,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn inspect_restore_generation(path: &Path) -> Result<u64> {
    let original = aw_platform::open_private_existing(path, false)?;
    drop(original);
    let c = db(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ))?;
    let value: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='restore_generation'",
        [],
        |row| row.get(0),
    ))?;
    let generation = value
        .parse::<u64>()
        .map_err(|_| Error::Denied("original SQLite restore generation is not valid"))?;
    if generation == 0 {
        return Err(Error::Denied(
            "original SQLite restore generation cannot be zero",
        ));
    }
    Ok(generation)
}

fn inspect_sqlite(path: &Path) -> Result<(String, i64, i64)> {
    let file = aw_platform::open_private_existing(path, false)?;
    drop(file);
    let connection = db(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ))?;
    let check: String = db(connection.query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
    if check != "ok" {
        return Err(Error::Denied("partial SQLite integrity check failed"));
    }
    let id: String = db(connection.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |r| r.get(0),
    ))?;
    let application: i64 = db(connection.pragma_query_value(None, "application_id", |r| r.get(0)))?;
    let schema: i64 = db(connection.pragma_query_value(None, "user_version", |r| r.get(0)))?;
    if schema != SQLITE_SCHEMA || application != SQLITE_APPLICATION {
        return Err(Error::Denied(
            "partial SQLite has unsupported schema/application",
        ));
    }
    Ok((id, application, schema))
}

fn capture_partial<F>(
    store: &SessionStore,
    parent: &Path,
    mut after_copy: F,
) -> Result<PartialCapture>
where
    F: FnMut(&Path, usize) -> Result<()>,
{
    if store.inner.mode != StoreMode::OwnerLock {
        return Err(Error::Denied(
            "partial composite staging requires exact operator owner lock",
        ));
    }
    let original_root = store.directory().ok_or(Error::Denied(
        "partial capture requires original on-disk Store",
    ))?;
    aw_platform::validate_private_dir(parent)?;
    let parent = parent.canonicalize()?;
    // A staging directory must never become an item in the source vault or a
    // child of the original Store while it is being copied.
    if parent.starts_with(original_root) || original_root.starts_with(&parent) {
        return Err(Error::Denied(
            "composite staging must be separate from original owner Store",
        ));
    }
    let lease = store.acquire_local_maintenance_lease("backup", Duration::from_secs(5 * 60))?;
    let result = (|| {
        // Do not expose private encrypted refs or plaintext: use the original
        // operator-only verification as the read-only active-custody gate.
        let current = store.verify_active_secret_custody(&lease)?;
        let partial = PartialCapture::new(&parent)?;
        let mut files = Vec::new();
        let mut copied = 0usize;
        // Same original maintenance lease: do NOT call SessionStore::backup,
        // which would attempt to reacquire the exact same exclusive SQL/vault
        // gate and deadlock. Reuse the original WAL-aware VACUUM INTO operation.
        let sqlite = partial.directory.join("sessions.sqlite3");
        let empty = aw_platform::open_private_new(&sqlite)?;
        drop(empty);
        {
            let c = store
                .inner
                .conn
                .lock()
                .map_err(|_| Error::Storage("store SQL mutex poisoned"))?;
            db(c.execute(
                "VACUUM INTO ?1",
                [sqlite
                    .to_str()
                    .ok_or(Error::Denied("non-UTF8 partial SQLite path"))?],
            ))?;
        }
        let staged_sqlite = aw_platform::open_private_existing(&sqlite, false)?;
        if staged_sqlite.metadata()?.len() > MAX_BYTES {
            return Err(Error::Limit("partial SQL snapshot exceeds size budget"));
        }
        staged_sqlite.sync_all()?;
        drop(staged_sqlite);
        let _ = inspect_sqlite(&sqlite)?;
        let mut sqlite_file = aw_platform::open_private_existing(&sqlite, false)?;
        let len = sqlite_file.metadata()?.len();
        let mut digest = Sha256::new();
        let copied_len = hash_limited(&mut sqlite_file, &mut digest, len)?;
        if copied_len != len {
            return Err(Error::Denied("partial SQLite source changed"));
        }
        let mut used = len;
        files.push(FileReceipt {
            path: "sessions.sqlite3".into(),
            length: len,
            sha256: format!("{:x}", digest.finalize()),
        });
        copied += 1;
        after_copy(&partial.directory, copied)?;

        {
            let mut capture_file = |source: &Path, relative: &str, max_size: u64| -> Result<()> {
                let proof = copy_private(
                    source,
                    &partial.directory.join(relative),
                    max_size,
                    relative,
                )?;
                used = used
                    .checked_add(proof.length)
                    .ok_or(Error::Limit("partial capture bytes overflow"))?;
                if used > MAX_BYTES || files.len() >= MAX_PRIVATE_ITEMS + 2 {
                    return Err(Error::Limit(
                        "partial capture exceeds operator snapshot budget",
                    ));
                }
                files.push(proof);
                copied += 1;
                after_copy(&partial.directory, copied)?; // private bounded test seam, no public API
                Ok(())
            };
            let source_vault = original_root.join("secrets");
            if source_vault.exists() {
                aw_platform::validate_private_dir(&source_vault)?;
                let vault = partial.directory.join("secrets");
                let mut dir = DirBuilder::new();
                dir.mode(0o700);
                dir.create(&vault)?;
                aw_platform::validate_private_dir(&vault)?;
                let source_master = source_vault.join("master.key");
                if source_master.exists() {
                    capture_file(&source_master, "secrets/master.key", 32)?;
                } else if current.active_unique != 0 || current.unreferenced_items != 0 {
                    return Err(Error::Denied(
                        "original missing master cannot stage encrypted custody",
                    ));
                }
                let source_items = source_vault.join("items");
                if source_items.exists() {
                    aw_platform::validate_private_dir(&source_items)?;
                    let dest_items = vault.join("items");
                    let mut builder = DirBuilder::new();
                    builder.mode(0o700);
                    builder.create(&dest_items)?;
                    let mut list = fs::read_dir(&source_items)?
                        .map(|item| item.map(|e| e.file_name()))
                        .collect::<std::io::Result<Vec<_>>>()?;
                    list.sort();
                    if list.len() > MAX_PRIVATE_ITEMS {
                        return Err(Error::Limit("too many original encrypted private items"));
                    }
                    for item in list {
                        let name = item
                            .to_str()
                            .ok_or(Error::Denied("non-UTF8 encrypted item name"))?;
                        if name.contains('/') || name == "." || name == ".." {
                            return Err(Error::Denied("invalid original item path"));
                        }
                        let relative = format!("secrets/items/{name}");
                        capture_file(&source_items.join(name), &relative, ITEM_LIMIT)?;
                    }
                    aw_platform::sync_parent(&dest_items.join("child"))?;
                }
                aw_platform::sync_parent(&vault.join("child"))?;
            }
        }
        let (store_id, app, schema) = inspect_sqlite(&sqlite)?;
        let manifest = PartialManifest {
            format: CAPTURE_ID.into(),
            complete: false,
            store_id,
            application_id: app,
            schema,
            active_items: current.active_unique,
            unreferenced_items: current.unreferenced_items,
            files,
        };
        let encoded = serde_json::to_vec(&manifest)
            .map_err(|_| Error::Storage("cannot encode internal partial manifest"))?;
        if encoded.len() > MAX_MANIFEST_BYTES {
            return Err(Error::Limit("partial manifest exceeds 8MiB"));
        }
        let manifest_path = partial.directory.join("manifest.partial.json");
        let mut file = aw_platform::open_private_new(&manifest_path)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        drop(file);
        aw_platform::sync_parent(&manifest_path)?;
        partial.verify()?;
        // The original SQL + vault maintenance token must remain the SAME
        // unexpired, exclusive owner lease through completion of all staged
        // bytes. A slow snapshot is rejected, never published as complete.
        if store.verify_active_secret_custody(&lease)? != current {
            return Err(Error::Denied(
                "original active custody changed while staging",
            ));
        }
        Ok(partial)
    })();
    let release = store.release_local_maintenance_lease(&lease);
    // Even a stage whose bytes were internally valid must never escape as a
    // successful artifact if its original maintenance fence failed to release.
    match (result, release) {
        (Ok(stage), Ok(())) => Ok(stage),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

/// AW-P6-10S: native Linux internal *offline* cold-restore only. A verified
/// COMPLETE V1 bundle is copied to a brand-new owner-private directory and
/// its original Process/generation/WriterEpoch authority is FENCED while the
/// private quarantine marker prevents ANY owner/Agent/Queue/Tool activation.
/// It is deliberately NOT a public operator method, CLI/SDK/RPC/MCP action or
/// automatic startup workflow. Activation requires a separately tested phase.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColdRestoreStep {
    AfterCopiedFile(usize),
    AfterOfflineFence,
    BeforeRename,
    AfterRenameBeforeParentSync,
}

#[cfg(target_os = "linux")]
fn cold_marker(store_id: &str, original_generation: u64, restored_generation: u64) -> String {
    format!(
        "aw-COLD-RESTORE-QUARANTINED-v1\nstore_id={store_id}\nsource_generation={original_generation}\nnew_generation={restored_generation}\n"
    )
}

#[cfg(target_os = "linux")]
fn require_absent_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Error::Denied(
            "cold restore target already exists; NEVER overwrite another owner State",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Io(error)),
    }
}

/// AW-P6-10T: source-independent, read-only inspection prerequisite.
/// Return only an internal verified identity; no runnable Process, Agent,
/// original Queue claim, Tool execution, credential key or activation grant.
#[cfg(target_os = "linux")]
pub(crate) fn verify_cold_for_inspection(
    published: &Path,
    quarantined: &Path,
) -> Result<(String, u64)> {
    let original = verify_complete_directory(published)?;
    let generation = original
        .restore_generation
        .checked_add(1)
        .ok_or(Error::Limit("quarantine generation exhausted"))?;
    verify_cold_quarantined_state(quarantined, &original, generation)?;
    Ok((original.store_id, generation))
}

#[cfg(target_os = "linux")]
fn verify_cold_quarantined_state(
    root: &Path,
    original: &CompleteManifest,
    expected_generation: u64,
) -> Result<()> {
    aw_platform::validate_private_dir(root)?;
    let mut marker_file =
        aw_platform::open_private_existing(&root.join(COLD_QUARANTINE_MARKER), false)?;
    let mut marker_text = String::new();
    (&mut marker_file)
        .take(1024)
        .read_to_string(&mut marker_text)?;
    if marker_text
        != cold_marker(
            &original.store_id,
            original.restore_generation,
            expected_generation,
        )
    {
        return Err(Error::Denied(
            "offline cold restore quarantine marker/identity changed",
        ));
    }
    // Original source-independent v33/AEAD verification is reused, not a new
    // decryption engine, signing key or hidden provider credential grant.
    let (store_id, app, schema) = inspect_sqlite(&root.join("sessions.sqlite3"))?;
    if store_id != original.store_id
        || app != original.application_id
        || schema != original.schema
        || inspect_restore_generation(&root.join("sessions.sqlite3"))? != expected_generation
    {
        return Err(Error::Denied(
            "offline cold restored SQLite identity/generation mismatched",
        ));
    }
    let c = db(Connection::open_with_flags(
        root.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ))?;
    // The filesystem marker and the persistent original SQLite denial record
    // must match the SAME original Store and fenced generation. Neither a
    // missing nor a spoofed value grants an owner/Process/replay authority.
    let sql_quarantine: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='cold_restore_quarantine'",
        [],
        |row| row.get(0),
    ))?;
    if sql_quarantine
        != crate::coordination::cold_quarantine_identity(
            &original.store_id,
            original.restore_generation,
            expected_generation,
        )
    {
        return Err(Error::Denied(
            "cold quarantine SQL identity was removed or altered",
        ));
    }
    let actual = secret_custody_inventory::verify_existing_secret_custody(&c, root)?;
    if actual.active_unique != original.active_items
        || actual.unreferenced_items != original.unreferenced_items
    {
        return Err(Error::Denied(
            "offline cold restore changed active encrypted custody",
        ));
    }
    // Exact copied master and ciphertext receipts must survive offline
    // generation-fence mutation too. SQLite is deliberately changed (new
    // generation, immutable writer quarantine) so it cannot match the old
    // archive hash; all separately encrypted files MUST still match exactly.
    for receipt in &original.files {
        if receipt.path == "sessions.sqlite3" {
            continue;
        }
        let mut file = aw_platform::open_private_existing(&root.join(&receipt.path), false)?;
        if file.metadata()?.len() != receipt.length {
            return Err(Error::Denied(
                "restored private item length changed from original complete manifest",
            ));
        }
        let mut digest = Sha256::new();
        let length = hash_limited(&mut file, &mut digest, receipt.length)?;
        if length != receipt.length || format!("{:x}", digest.finalize()) != receipt.sha256 {
            return Err(Error::Denied(
                "restored private item differs from original complete manifest",
            ));
        }
    }
    // A cold State has exactly its private SQLite image, quarantine marker,
    // and the original secrets/ tree. No copied owner.lock, SQLite WAL, extra
    // executables, unchecked child directories or hidden token blobs.
    let secrets = root.join("secrets");
    let mut root_expected = BTreeSet::from([
        "sessions.sqlite3".to_owned(),
        COLD_QUARANTINE_MARKER.to_owned(),
    ]);
    if secrets.exists() {
        root_expected.insert("secrets".into());
        let mut secret_expected = BTreeSet::new();
        if original
            .files
            .iter()
            .any(|f| f.path == "secrets/master.key")
        {
            secret_expected.insert("master.key".to_owned());
        }
        let items = secrets.join("items");
        if items.exists() {
            secret_expected.insert("items".to_owned());
            let desired = original
                .files
                .iter()
                .filter_map(|f| f.path.strip_prefix("secrets/items/").map(str::to_owned))
                .collect::<BTreeSet<_>>();
            require_exact_entries(&items, &desired)?;
        } else if original
            .files
            .iter()
            .any(|f| f.path.starts_with("secrets/items/"))
        {
            return Err(Error::Denied(
                "restored original encrypted directory is missing",
            ));
        }
        require_exact_entries(&secrets, &secret_expected)?;
    } else if original
        .files
        .iter()
        .any(|f| f.path.starts_with("secrets/"))
    {
        return Err(Error::Denied(
            "restored original private SecretStore directory is missing",
        ));
    }
    require_exact_entries(root, &root_expected)?;
    let quarantined_writers: i64 = db(c.query_row(
        "SELECT count(*) FROM workspace_writer_state w WHERE w.state='read_only' AND NOT EXISTS(
           SELECT 1 FROM workspace_writer_events e
           WHERE e.workspace_id=w.workspace_id AND e.new_writer_epoch=w.writer_epoch
             AND e.reason='cold_restore_quarantine')",
        [],
        |row| row.get(0),
    ))?;
    if quarantined_writers != 0 {
        return Err(Error::Denied(
            "restored read-only writer lacks original immutable epoch fence",
        ));
    }
    let active_processes: i64 = db(c.query_row(
        "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
        [],
        |row| row.get(0),
    ))?;
    let active_writers: i64 = db(c.query_row(
        "SELECT count(*) FROM workspace_writer_state WHERE state='active'",
        [],
        |row| row.get(0),
    ))?;
    let active_maintenance: i64 = db(c.query_row(
        "SELECT count(*) FROM local_maintenance_lease WHERE state='active'",
        [],
        |row| row.get(0),
    ))?;
    let active_leases: i64 = db(c.query_row(
        "SELECT count(*) FROM local_coordination_leases WHERE state='active'",
        [],
        |row| row.get(0),
    ))?;
    if active_processes != 0 || active_writers != 0 || active_maintenance != 0 || active_leases != 0
    {
        return Err(Error::Denied(
            "quarantined cold restore retained executable Process/Writer/lease authority",
        ));
    }
    let foreign_keys: i64 = db(c.query_row(
        "SELECT count(*) FROM pragma_foreign_key_check",
        [],
        |row| row.get(0),
    ))?;
    if foreign_keys != 0 {
        return Err(Error::Denied("quarantined SQLite has foreign-key errors"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn restore_complete_cold_quarantined<F>(
    published: &Path,
    destination: &Path,
    mut boundary: F,
) -> Result<PathBuf>
where
    F: FnMut(ColdRestoreStep, &Path) -> Result<()>,
{
    // First VERIFY the source independently while touching NO destination.
    // Missing active SecretStore item/key, tampered ciphertext, bad original
    // Queue digest, inconsistent schema or incomplete bundle fail here.
    let original = verify_complete_directory(published)?;
    let final_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::Denied(
            "cold restore requires a single exact private destination basename",
        ))?;
    if !final_name.starts_with("aw-restored-")
        || final_name.len() > 96
        || !final_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Error::Denied(
            "cold restore name must be bounded ASCII aw-restored-*",
        ));
    }
    let parent = destination
        .parent()
        .ok_or(Error::Denied("cold restore missing private parent"))?;
    aw_platform::validate_private_dir(parent)?;
    let parent = parent.canonicalize()?;
    let destination = parent.join(final_name);
    if destination.starts_with(published) || published.starts_with(&destination) {
        return Err(Error::Denied(
            "cannot restore into original published bundle",
        ));
    }
    require_absent_path(&destination)?;

    // Stage inside EXACT final parent for a strictly same-FS RENAME_NOREPLACE.
    // Reuse the original P/Q private RAII stage ownership: it never deletes a
    // preexisting destination and disarms only after kernel rename success.
    let mut stage = PartialCapture::new(&parent)?;
    let marker_path = stage.directory.join(COLD_QUARANTINE_MARKER);
    let mut marker = aw_platform::open_private_new(&marker_path)?;
    // Source generation is not a grant; quarantine increments it explicitly.
    let generation = original
        .restore_generation
        .checked_add(1)
        .ok_or(Error::Limit("offline cold restore generation exhausted"))?;
    marker.write_all(
        cold_marker(&original.store_id, original.restore_generation, generation).as_bytes(),
    )?;
    marker.sync_all()?;
    drop(marker);

    let mut copied = 0usize;
    let mut total = 0u64;
    for file in &original.files {
        let relative = Path::new(&file.path);
        // The completed V1 validator already checked every component, but
        // repeat these critical checks before writing into a new State.
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
            || !(file.path == "sessions.sqlite3"
                || file.path == "secrets/master.key"
                || file.path.starts_with("secrets/items/"))
        {
            return Err(Error::Denied(
                "unsafe original composite entry for cold restore",
            ));
        }
        let staged_path = stage.directory.join(relative);
        let parent_dir = staged_path
            .parent()
            .ok_or(Error::Denied("cold restored file has no parent"))?;
        if !parent_dir.exists() {
            // Only original fixed private source layout: secrets/ and items/.
            let secrets = stage.directory.join("secrets");
            if !secrets.exists() {
                let mut builder = DirBuilder::new();
                builder.mode(0o700);
                builder.create(&secrets)?;
            }
            let items = stage.directory.join("secrets/items");
            if file.path.starts_with("secrets/items/") && !items.exists() {
                let mut builder = DirBuilder::new();
                builder.mode(0o700);
                builder.create(&items)?;
            }
        }
        let receipt = copy_private(
            &published.join(relative),
            &staged_path,
            if file.path == "sessions.sqlite3" {
                MAX_BYTES
            } else if file.path == "secrets/master.key" {
                32
            } else {
                ITEM_LIMIT
            },
            &file.path,
        )?;
        if receipt.path != file.path
            || receipt.length != file.length
            || receipt.sha256 != file.sha256
        {
            return Err(Error::Denied(
                "cold restore source file changed during copy",
            ));
        }
        total = total
            .checked_add(receipt.length)
            .ok_or(Error::Limit("restore byte overflow"))?;
        if total > MAX_BYTES {
            return Err(Error::Limit("offline restored files exceed bounded 512MiB"));
        }
        copied += 1;
        boundary(ColdRestoreStep::AfterCopiedFile(copied), &stage.directory)?;
    }
    if copied != original.files.len() {
        return Err(Error::Denied("cold restore omitted a recorded file"));
    }
    // File fsync is insufficient for newly created nested directory entries.
    // Sync each new private directory under the SAME stage BEFORE the final
    // kernel no-overwrite rename, preserving the original root/key/item tree.
    let secrets = stage.directory.join("secrets");
    if secrets.exists() {
        let items = secrets.join("items");
        if items.exists() {
            aw_platform::sync_parent(&items.join("directory-entry-sync"))?;
        }
        aw_platform::sync_parent(&secrets.join("directory-entry-sync"))?;
    }
    aw_platform::sync_parent(&stage.directory.join("directory-entry-sync"))?;
    // The full stage is verified before any modification to the STAGED SQL.
    // Quarantine's extra private marker is its only additional State file.
    let staged_sqlite = stage.directory.join("sessions.sqlite3");
    let (staged_id, app, schema) = inspect_sqlite(&staged_sqlite)?;
    if staged_id != original.store_id
        || app != original.application_id
        || schema != original.schema
        || inspect_restore_generation(&staged_sqlite)? != original.restore_generation
    {
        return Err(Error::Denied(
            "cold restore snapshot identity changed before generation fencing",
        ));
    }
    {
        // Only the NEW private copy is opened read-write, NEVER the published
        // source or a runnable owner Store. DELETE journal mode avoids orphan
        // WAL/-shm sidecars when publishing a pristine cold State directory.
        let mut connection = db(Connection::open_with_flags(
            &staged_sqlite,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ))?;
        db(connection.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
             PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;",
        ))?;
        let next = crate::coordination::prepare_offline_cold_restore(
            &mut connection,
            &original.store_id,
            original.restore_generation,
        )?;
        if next != generation {
            return Err(Error::Denied(
                "original offline restore process generation changed",
            ));
        }
        let quick: String = db(connection.query_row("PRAGMA quick_check", [], |row| row.get(0)))?;
        let fk: i64 = db(connection.query_row(
            "SELECT count(*) FROM pragma_foreign_key_check",
            [],
            |row| row.get(0),
        ))?;
        if quick != "ok" || fk != 0 {
            return Err(Error::Denied(
                "offline staged original SQLite integrity failed after fencing",
            ));
        }
        drop(connection);
    }
    let original_sqlite = aw_platform::open_private_existing(&staged_sqlite, false)?;
    original_sqlite.sync_all()?;
    aw_platform::sync_parent(&staged_sqlite)?;
    // A real SQLite journal/WAL sidecar is never accepted as an ignored extra
    // authority when installing a cold State. It must have been checkpointed.
    for name in [
        "sessions.sqlite3-wal",
        "sessions.sqlite3-shm",
        "sessions.sqlite3-journal",
    ] {
        if fs::symlink_metadata(stage.directory.join(name)).is_ok() {
            return Err(Error::Denied(
                "offline staged SQLite left unverified journal sidecar",
            ));
        }
    }
    aw_platform::sync_parent(&marker_path)?;
    verify_cold_quarantined_state(&stage.directory, &original, generation)?;
    boundary(ColdRestoreStep::AfterOfflineFence, &stage.directory)?;
    boundary(ColdRestoreStep::BeforeRename, &stage.directory)?;
    // The R kernel NO-REPLACE publication helper is reused, but this is a
    // RESTORED QUARANTINE, not a completed backup or runnable owner Store.
    rename_private_no_replace(&stage.directory, &destination)?;
    stage.published = true;
    boundary(ColdRestoreStep::AfterRenameBeforeParentSync, &destination)?;
    aw_platform::sync_parent(&destination)?;
    verify_cold_quarantined_state(&destination, &original, generation)?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider_secret_ref, ActorQueueRequest, QueueTarget, SecretStore, SessionBinding,
        SessionStore,
    };
    use aw_agent_core::PrincipalId;
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, PermissionsExt},
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct TempRoot(PathBuf);
    impl TempRoot {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "aw-p6-10p-internal-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            builder.create(&path).unwrap();
            Self(path)
        }
        fn child(&self, name: &str) -> PathBuf {
            let child = self.0.join(name);
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            builder.create(&child).unwrap();
            child
        }
    }
    impl Drop for TempRoot {
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
    fn create_encrypted_queue(store: &SessionStore) -> String {
        let actor = PrincipalId::new("local").unwrap();
        let (session, branch) = store
            .create(&actor, "private pending capture", binding())
            .unwrap();
        let writer = store
            .workspace_writer(&actor, None, &session.workspace_id)
            .unwrap();
        store
            .actor_queue_enqueue(
                &actor,
                ActorQueueRequest {
                    request_id: "captured-original-queue".into(),
                    session_id: session.id,
                    branch_id: branch.id,
                    recipient_site: "local".into(),
                    sender: "r9g1a1g1".into(),
                    message_id: 1,
                    writer_epoch: writer.writer_epoch,
                    target: QueueTarget::Auto,
                    priority: 0,
                },
                "ORIGINAL_SYNTHETIC_ENCRYPTED_PENDING_INPUT",
            )
            .unwrap()
            .payload_ref
    }

    #[test]
    fn aw_p6_10p_genuine_private_stage_captures_sqlite_vault_and_never_publishes() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("private-target");
        let store = SessionStore::open(&state).unwrap();
        let active = create_encrypted_queue(&store);
        let vault = store.local_secret_store().unwrap();
        let orphan = provider_secret_ref("fixture", "retired-only", 1).unwrap();
        vault
            .put(&orphan, b"RETIRED_AND_UNREFERENCED_SYNTHETIC_ITEM")
            .unwrap();
        let previous = target.join("earlier-good-backup-preserved");
        fs::write(&previous, b"NEVER_REMOVE_OR_OVERWRITE_THIS_FILE").unwrap();
        fs::set_permissions(&previous, fs::Permissions::from_mode(0o600)).unwrap();
        let mut stages = Vec::new();
        let stage = capture_partial(&store, &target, |dir, copied| {
            stages.push((dir.to_owned(), copied));
            Ok(())
        })
        .unwrap();
        assert!(stage.directory.to_string_lossy().ends_with(".partial"));
        let manifest = stage.verify().unwrap();
        assert!(!manifest.complete && manifest.format == CAPTURE_ID);
        assert_eq!(manifest.active_items, 1);
        assert_eq!(manifest.unreferenced_items, 1);
        assert_eq!(
            manifest.files.len(),
            4,
            "SQLite + master + both authenticated write-once items"
        );
        assert_eq!(stages.len(), 4);
        assert_eq!(
            fs::read(&previous).unwrap(),
            b"NEVER_REMOVE_OR_OVERWRITE_THIS_FILE"
        );
        let staged_vault = crate::LocalSecretStore::open(&stage.directory).unwrap();
        assert_eq!(
            staged_vault.get(&active).unwrap().as_slice(),
            b"ORIGINAL_SYNTHETIC_ENCRYPTED_PENDING_INPUT"
        );
        assert_eq!(
            staged_vault.get(&orphan).unwrap().as_slice(),
            b"RETIRED_AND_UNREFERENCED_SYNTHETIC_ITEM"
        );
        let saved_path = stage.directory.clone();
        drop(staged_vault);
        drop(stage);
        assert!(
            !saved_path.exists(),
            "the private partial never becomes a complete backup"
        );
        assert_eq!(
            fs::read(previous).unwrap(),
            b"NEVER_REMOVE_OR_OVERWRITE_THIS_FILE"
        );
        vault.delete(&orphan).unwrap(); // original owner lease released after staged read
    }

    #[test]
    fn aw_p6_10p_failed_mid_capture_removes_only_owned_partial_and_releases_vault() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        let active = create_encrypted_queue(&store);
        let original = store.local_secret_store().unwrap();
        let previous = target.join("previous-archive");
        fs::write(&previous, b"KEEP_PREVIOUS_ARCHIVE_BYTE_FOR_BYTE").unwrap();
        fs::set_permissions(&previous, fs::Permissions::from_mode(0o600)).unwrap();
        for fail_after in 1..=3 {
            let mut staged_root = None;
            let result = capture_partial(&store, &target, |dir, count| {
                staged_root = Some(dir.to_owned());
                // Fail after real SQLite, then original master, then the
                // original encrypted item in independent staging attempts.
                if count == fail_after {
                    return Err(Error::Denied("injected staged file failure"));
                }
                Ok(())
            });
            assert!(result.is_err());
            assert!(
                !staged_root.unwrap().exists(),
                "failed scoped partial directory must be gone"
            );
            assert_eq!(
                fs::read(&previous).unwrap(),
                b"KEEP_PREVIOUS_ARCHIVE_BYTE_FOR_BYTE"
            );
            assert!(original.get(&active).is_ok());
        }
        original.delete(&active).unwrap(); // original exclusive maintenance released
        assert!(!original.exists(&active).unwrap());
    }

    #[test]
    fn aw_p6_10p_bad_parent_and_missing_active_ciphertext_cannot_stage() {
        use std::os::unix::fs::symlink;
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("private-target");
        let store = SessionStore::open(&state).unwrap();
        let active = create_encrypted_queue(&store);
        let alias = root.0.join("operator-symlink");
        symlink(&target, &alias).unwrap();
        assert!(capture_partial(&store, &alias, |_, _| Ok(())).is_err());
        assert!(capture_partial(&store, &state, |_, _| Ok(())).is_err());
        let vault = store.local_secret_store().unwrap();
        vault.delete(&active).unwrap();
        assert!(capture_partial(&store, &target, |_, _| Ok(())).is_err());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        assert!(store
            .acquire_local_maintenance_lease("backup", Duration::from_secs(60))
            .and_then(|lease| store.release_local_maintenance_lease(&lease))
            .is_ok());
    }

    #[test]
    fn aw_p6_10p_original_fence_blocks_new_mutations_during_partial_capture() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        let existing = create_encrypted_queue(&store);
        let vault = store.local_secret_store().unwrap();
        let future = provider_secret_ref("fixture", "not-during-snapshot", 1).unwrap();
        let mut observed = false;
        let stage = capture_partial(&store, &target, |_, copied| {
            if copied == 1 {
                assert!(vault
                    .put(&future, b"NO_NEW_PRIVATE_BYTES_DURING_CAPTURE")
                    .is_err());
                assert!(vault.delete(&existing).is_err());
                observed = true;
            }
            Ok(())
        })
        .unwrap();
        assert!(observed);
        assert_eq!(stage.verify().unwrap().active_items, 1);
        drop(stage);
        vault.put(&future, b"AFTER_CAPTURE_ALLOWED").unwrap();
        vault.delete(&future).unwrap();
    }

    #[test]
    fn aw_p6_10p_partial_manifest_cannot_claim_complete_or_omit_copied_files() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&store);
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        let original = stage.verify().unwrap();
        assert!(!original.complete);
        let path = stage.directory.join("manifest.partial.json");
        let mut forged = original.clone();
        forged.complete = true;
        fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(
            stage.verify().is_err(),
            "a partial manifest must never be marked complete"
        );
        forged = original.clone();
        forged
            .files
            .retain(|file| !file.path.starts_with("secrets/items/"));
        fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(
            stage.verify().is_err(),
            "original encrypted items cannot be omitted from receipts"
        );
        fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        stage.verify().unwrap();
        let unexpected = stage.directory.join("secrets/items/extra-private-item.bin");
        fs::write(&unexpected, b"UNATTESTED_EXTRA_FILE").unwrap();
        fs::set_permissions(&unexpected, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            stage.verify().is_err(),
            "a private but unreceipted extra item cannot be ignored"
        );
        fs::remove_file(&unexpected).unwrap();
        assert!(stage.verify().is_ok());
    }

    #[test]
    fn aw_p6_10q_independent_process_verifies_capture_after_original_source_is_removed() {
        const ROOT: &str = "AW_P6_10Q_STAGED_INDEPENDENT_ROOT";
        const EXPECT_FAILURE: &str = "AW_P6_10Q_VERIFY_EXPECT_FAILURE";
        if let Some(path) = std::env::var_os(ROOT) {
            // This is a SECOND actual OS process with no SessionStore, owner
            // lock, maintenance token or access to the now-removed source.
            let outcome = verify_partial_directory(Path::new(&path));
            if std::env::var_os(EXPECT_FAILURE).is_some() {
                assert!(
                    outcome.is_err(),
                    "forged staged image cannot authenticate independently"
                );
            } else {
                let manifest = outcome.unwrap();
                assert!(!manifest.complete);
                assert_eq!(manifest.active_items, 1);
            }
            return;
        }
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        let active = create_encrypted_queue(&store);
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        drop(store);
        fs::remove_dir_all(&state).unwrap();
        assert!(
            !state.exists(),
            "original Store and master physically absent before independent verification"
        );
        let exercise = |expect_failure: bool| {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.arg("--exact")
                .arg("composite_staging::tests::aw_p6_10q_independent_process_verifies_capture_after_original_source_is_removed")
                .arg("--nocapture")
                .env(ROOT, &stage.directory);
            if expect_failure {
                child.env(EXPECT_FAILURE, "yes");
            }
            let result = child.output().unwrap();
            assert!(
                result.status.success(),
                "standalone child must enforce expectation; status={} stdout={} stderr={}",
                result.status,
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        };
        exercise(false);
        let relative = format!("secrets/items/{active}.bin");
        let path = stage.directory.join(&relative);
        let mut tampered = fs::read(&path).unwrap();
        *tampered.last_mut().unwrap() ^= 0x55;
        fs::write(&path, &tampered).unwrap();
        let manifest_file = stage.directory.join("manifest.partial.json");
        let mut manifest: PartialManifest =
            serde_json::from_slice(&fs::read(&manifest_file).unwrap()).unwrap();
        manifest
            .files
            .iter_mut()
            .find(|file| file.path == relative)
            .unwrap()
            .sha256 = format!("{:x}", Sha256::digest(tampered));
        fs::write(manifest_file, serde_json::to_vec(&manifest).unwrap()).unwrap();
        exercise(true);
    }

    #[test]
    fn aw_p6_10q_real_process_exits_mid_capture_and_quarantines_unpublished_partial() {
        const SOURCE: &str = "AW_P6_10Q_CRASH_SOURCE";
        const DEST: &str = "AW_P6_10Q_CRASH_DEST";
        if let Some(state) = std::env::var_os(SOURCE) {
            let owner = SessionStore::open(Path::new(&state)).unwrap();
            let target = PathBuf::from(std::env::var_os(DEST).unwrap());
            let _ = capture_partial(&owner, &target, |_, count| {
                if count == 2 {
                    // No destructors or SQL maintenance-release callback.
                    // Real OS exit leaves a quarantined partial without a
                    // manifest. It must NEVER be interpreted as recoverable.
                    std::process::exit(53);
                }
                Ok(())
            });
            panic!("mid-capture child never reached the physical file boundary");
        }
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        let original = create_encrypted_queue(&store);
        drop(store);
        let previous = target.join("previous-user-backup");
        fs::write(&previous, b"PRESERVE_ACTUAL_PREVIOUS_BACKUP").unwrap();
        fs::set_permissions(&previous, fs::Permissions::from_mode(0o600)).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("composite_staging::tests::aw_p6_10q_real_process_exits_mid_capture_and_quarantines_unpublished_partial")
            .arg("--nocapture")
            .env(SOURCE, &state).env(DEST, &target)
            .output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(53),
            "actual child must exit mid-capture: stdout={} stderr={}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let dirs = fs::read_dir(&target)
            .unwrap()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if entry.file_name().to_string_lossy().ends_with(".partial") {
                    Some(entry.path())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            dirs.len(),
            1,
            "the crash leaves exactly one private quarantined incomplete stage"
        );
        assert!(
            verify_partial_directory(&dirs[0]).is_err(),
            "an incomplete stage from a dead owner process cannot verify as an archive"
        );
        assert_eq!(
            fs::read(previous).unwrap(),
            b"PRESERVE_ACTUAL_PREVIOUS_BACKUP"
        );
        // The operating system closes the dead owner's vault lock. No Queue
        // or Agent is replayed; original bytes remain obtainable read-only.
        let resumed = crate::LocalSecretStore::open(&state).unwrap();
        assert_eq!(
            resumed.get(&original).unwrap().as_slice(),
            b"ORIGINAL_SYNTHETIC_ENCRYPTED_PENDING_INPUT"
        );
    }

    #[test]
    fn aw_p6_10q_real_process_exits_after_complete_partial_without_publishing() {
        const SOURCE: &str = "AW_P6_10Q_COMPLETE_SOURCE";
        const DEST: &str = "AW_P6_10Q_COMPLETE_DEST";
        if let Some(state) = std::env::var_os(SOURCE) {
            let owner = SessionStore::open(Path::new(&state)).unwrap();
            let target = PathBuf::from(std::env::var_os(DEST).unwrap());
            let _partial = capture_partial(&owner, &target, |_, _| Ok(())).unwrap();
            std::process::exit(54); // No Drop: complete partial remains incomplete/not published.
        }
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        drop(owner);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("composite_staging::tests::aw_p6_10q_real_process_exits_after_complete_partial_without_publishing")
            .arg("--nocapture")
            .env(SOURCE, &state).env(DEST, &target)
            .output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(54),
            "real child must exit after validated staging without scope Drop"
        );
        let stages = fs::read_dir(&target)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(stages.len(), 1);
        assert!(stages[0].to_string_lossy().ends_with(".partial"));
        let proof = verify_partial_directory(&stages[0]).unwrap();
        assert!(
            !proof.complete,
            "even a fully attested private stage is not a published backup"
        );
        assert_eq!(proof.active_items, 1);
    }

    #[test]
    fn aw_p6_10q_rehashed_sqlite_audit_cannot_relabel_active_queue_body() {
        // A coherent-looking rewritten SQLite file AND updated manifest SHA
        // are not permission to redefine the immutable queued Input bytes.
        // Q independently checks the STAGED v33 Queue digest against the
        // STAGED original authenticated ciphertext without any source Store.
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&store);
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        stage.verify().unwrap();
        let sqlite = stage.directory.join("sessions.sqlite3");
        let connection = Connection::open(&sqlite).unwrap();
        let original: String = connection
            .query_row(
                "SELECT data_json FROM audit WHERE action='actor.queue' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut fact: serde_json::Value = serde_json::from_str(&original).unwrap();
        fact["payload_sha256"] = serde_json::Value::String("f".repeat(64));
        // Test-only simulated offline rewrite of a *copied* SQLite image:
        // remove the staged copy's immutable-update trigger to model fully
        // replaced snapshot bytes. Real original v33 sources retain it.
        connection
            .execute("DROP TRIGGER audit_immutable_update", [])
            .unwrap();
        connection
            .execute(
                "UPDATE audit SET data_json=?1 WHERE seq=(SELECT seq FROM audit
             WHERE action='actor.queue' ORDER BY seq DESC LIMIT 1)",
                [serde_json::to_string(&fact).unwrap()],
            )
            .unwrap();
        drop(connection);
        let manifest_file = stage.directory.join("manifest.partial.json");
        let mut manifest: PartialManifest =
            serde_json::from_slice(&fs::read(&manifest_file).unwrap()).unwrap();
        manifest
            .files
            .iter_mut()
            .find(|file| file.path == "sessions.sqlite3")
            .unwrap()
            .sha256 = format!("{:x}", Sha256::digest(fs::read(&sqlite).unwrap()));
        fs::write(manifest_file, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            stage.verify().is_err(),
            "SQLite audit and file hashes cannot silently redefine a queued Input body"
        );
    }

    #[test]
    fn aw_p6_10q_partial_manifest_counts_cannot_understate_staged_active_custody() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&store);
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        let manifest_file = stage.directory.join("manifest.partial.json");
        let mut manifest: PartialManifest =
            serde_json::from_slice(&fs::read(&manifest_file).unwrap()).unwrap();
        assert_eq!(manifest.active_items, 1);
        manifest.active_items = 0;
        manifest.unreferenced_items = 1;
        fs::write(manifest_file, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            stage.verify().is_err(),
            "private file-count agreement is insufficient if SQL still has active custody"
        );
    }

    #[cfg(target_os = "linux")]
    fn aw_p6_10w7_real_cold_fixture() -> (TempRoot, PathBuf, PathBuf) {
        let root = TempRoot::new();
        let source = root.child("w7-original-store");
        let bundles = root.child("w7-composite-bundles");
        let private_parent = root.child("w7-cold-parent");
        let owner = SessionStore::open(&source).unwrap();
        create_encrypted_queue(&owner);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-w7-real-quarantine", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let cold = private_parent.join("aw-restored-w7-quarantined-state");
        restore_complete_cold_quarantined(&archive, &cold, |_, _| Ok(())).unwrap();
        (root, archive, cold)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10w7_original_cold_snapshot_is_nonexecuting_and_uses_real_quarantine() {
        let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let result = crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot(
            &archive, &cold,
        )
        .unwrap();
        assert_eq!(
            result.preparation.generation,
            inspect_restore_generation(&cold.join("sessions.sqlite3")).unwrap()
        );
        assert!(result.preparation.obligations.iter().any(|o| o.kind
            == crate::cold_effect_reconciliation::EvidenceClass::QueuePending
            && o.count == 1));
        assert_eq!(result.actor.len(), 1);
        assert!(!result.preparation.runnable);
        assert!(!result.preparation.permits_original_effect_replay);
        assert!(SessionStore::open(&cold).is_err());
    }

    // Keep the large original composite backup fixtures in one place, while
    // W8's independent original-owner review regressions remain a small file.
    #[cfg(target_os = "linux")]
    include!("composite_staging_w8_tests.rs");

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10w7_replaced_same_content_quarantine_marker_is_denied() {
        let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let marker = cold.join(COLD_QUARANTINE_MARKER);
        let original = fs::read(&marker).unwrap();
        let result =
            crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot_with_hook(
                &archive,
                &cold,
                || {
                    let replaced = cold.parent().unwrap().join("original-marker-moved-aside");
                    fs::rename(&marker, &replaced).unwrap();
                    fs::write(&marker, &original).unwrap();
                    fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
                    Ok(())
                },
            );
        assert!(
            result.is_err(),
            "replaced marker with identical bytes is NOT a stable inspection"
        );
        // The marker's copied content must not turn restored State executable.
        assert!(SessionStore::open(&cold).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10w7_replaced_same_content_sqlite_is_denied() {
        let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let sqlite = cold.join("sessions.sqlite3");
        let result =
            crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot_with_hook(
                &archive,
                &cold,
                || {
                    let replacement = cold
                        .parent()
                        .unwrap()
                        .join("same-bytes-replacement.sqlite3");
                    fs::copy(&sqlite, &replacement).unwrap();
                    fs::rename(&replacement, &sqlite).unwrap();
                    Ok(())
                },
            );
        assert!(
            result.is_err(),
            "identical SQL bytes under a NEW inode are NOT the pinned original file"
        );
        assert!(SessionStore::open(&cold).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_review_transient_sqlite_swap_restored_before_postcheck_is_denied() {
        // A concurrent local actor swaps in a same-byte copy and returns the
        // original inode before inspection returns. The held original root
        // directory's ctime/mtime and descriptor identity must still expose
        // the mutation; this is not a privileged-root-host certification.
        let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let sqlite = cold.join("sessions.sqlite3");
        let parking = cold.parent().unwrap().join("original-sqlite-parked");
        let replacement = cold.parent().unwrap().join("transient-sqlite-copy");
        let inspected =
            crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot_with_hook(
                &archive,
                &cold,
                || {
                    fs::copy(&sqlite, &replacement).unwrap();
                    fs::rename(&sqlite, &parking).unwrap();
                    fs::rename(&replacement, &sqlite).unwrap();
                    // Return original file to the exact same filesystem path.
                    fs::remove_file(&sqlite).unwrap();
                    fs::rename(&parking, &sqlite).unwrap();
                    Ok(())
                },
            );
        assert!(
            inspected.is_err(),
            "transient filename swap must be exposed by anchored parent metadata"
        );
        assert!(SessionStore::open(&cold).is_err());
        assert!(verify_cold_for_inspection(&archive, &cold).is_ok(),
            "the original cold archive is still valid but the in-flight inspection must be rejected");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_offline_restore_requires_quarantine_and_revokes_old_writer_epoch() {
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let target_parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        let original_ref = create_encrypted_queue(&owner);
        let original_session = {
            let actor = PrincipalId::new("local").unwrap();
            owner.list(&actor).unwrap().into_iter().next().unwrap()
        };
        let actor = PrincipalId::new("local").unwrap();
        let writer_before = owner
            .workspace_writer(&actor, None, &original_session.workspace_id)
            .unwrap();
        let original_generation = owner.local_process_info().unwrap().store_generation;
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-for-cold-restore", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        // Physically remove the source: the cold restore must be independent
        // of the old process, owner.lock and master.key.
        fs::remove_dir_all(&source).unwrap();
        let target = target_parent.join("aw-restored-new-owner-state");
        assert!(!target.exists());
        let restored =
            restore_complete_cold_quarantined(&published, &target, |_, _| Ok(())).unwrap();
        assert_eq!(restored, target);
        assert!(restored.join(".aw-cold-restore-pending").is_file());
        assert!(
            SessionStore::open(&restored).is_err(),
            "a copied original Store cannot register an Agent/process before operator activation"
        );
        assert!(
            SessionStore::open_trusted_local_direct(
                &restored,
                crate::TrustedLocalDirectOptions {
                    heartbeat_ttl: Duration::from_secs(6)
                }
            )
            .is_err(),
            "unapproved cold Store also cannot open in trusted-local direct mode"
        );
        let cloned = crate::LocalSecretStore::open(&restored).unwrap();
        assert_eq!(
            cloned.get(&original_ref).unwrap().as_slice(),
            b"ORIGINAL_SYNTHETIC_ENCRYPTED_PENDING_INPUT"
        );
        let c = Connection::open_with_flags(
            restored.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let generation = inspect_restore_generation(&restored.join("sessions.sqlite3")).unwrap();
        assert_eq!(generation, original_generation + 1);
        let (mode, epoch): (String, u64) = c
            .query_row(
                "SELECT state,writer_epoch FROM workspace_writer_state WHERE workspace_id=?1",
                [&original_session.workspace_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(mode, "read_only");
        assert!(epoch > writer_before.writer_epoch);
        let still_active: i64 = c
            .query_row(
                "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            still_active, 0,
            "offline cold restore must not create a runnable owner process"
        );
        assert_eq!(
            verify_complete_directory(&published)
                .unwrap()
                .restore_generation,
            original_generation,
            "cold restore must not mutate the original published complete backup"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_real_child_process_exit_before_or_after_atomic_quarantine_publication() {
        const BUNDLE: &str = "AW_P6_10S_EXIT_BUNDLE";
        const TARGET: &str = "AW_P6_10S_EXIT_TARGET";
        const MODE: &str = "AW_P6_10S_EXIT_MODE";
        if let Some(archive) = std::env::var_os(BUNDLE) {
            let target = PathBuf::from(std::env::var_os(TARGET).unwrap());
            let mode = std::env::var(MODE).unwrap();
            let fail_when = if mode == "before" {
                ColdRestoreStep::BeforeRename
            } else {
                ColdRestoreStep::AfterRenameBeforeParentSync
            };
            let _ = restore_complete_cold_quarantined(Path::new(&archive), &target, |step, _| {
                if step == fail_when {
                    // Actual process exit WITHOUT Rust Drop or a managed
                    // error cleanup. No natural power-loss claim is made.
                    std::process::exit(if mode == "before" { 81 } else { 82 });
                }
                Ok(())
            });
            panic!("child did not reach physical cold-restore rename boundary");
        }
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let before_parent = root.child("before-target-parent");
        let after_parent = root.child("after-target-parent");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let original_gen = owner.local_process_info().unwrap().store_generation;
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-for-process-exit", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        fs::remove_dir_all(&state).unwrap();
        for (mode, parent, expected_exit) in
            [("before", &before_parent, 81), ("after", &after_parent, 82)]
        {
            let destination = parent.join(format!("aw-restored-{mode}-exit"));
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("composite_staging::tests::aw_p6_10s_real_child_process_exit_before_or_after_atomic_quarantine_publication")
                .arg("--nocapture")
                .env(BUNDLE, &archive).env(TARGET, &destination).env(MODE, mode)
                .output().unwrap();
            assert_eq!(
                result.status.code(),
                Some(expected_exit),
                "native child did not exit at {mode} rename boundary: stdout={} stderr={}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            if mode == "before" {
                assert!(
                    !destination.exists(),
                    "pre-rename exit must never publish State"
                );
                let leftover = fs::read_dir(parent)
                    .unwrap()
                    .map(|v| v.unwrap().path())
                    .collect::<Vec<_>>();
                assert_eq!(leftover.len(), 1);
                assert!(leftover[0].to_string_lossy().ends_with(".partial"));
                assert!(leftover[0].join(COLD_QUARANTINE_MARKER).is_file());
                assert!(
                    SessionStore::open(&leftover[0]).is_err(),
                    "crash-left partial with a marker cannot activate a copied owner"
                );
            } else {
                assert!(destination.join(COLD_QUARANTINE_MARKER).is_file());
                assert!(SessionStore::open(&destination).is_err());
                assert_eq!(
                    inspect_restore_generation(&destination.join("sessions.sqlite3")).unwrap(),
                    original_gen + 1
                );
                assert_eq!(fs::read_dir(parent).unwrap().count(), 1);
            }
            assert!(
                verify_complete_directory(&archive).is_ok(),
                "neither process-exit boundary may mutate the original published archive"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn aw_p6_10s_original_unknown_paid_terminal_remains_unknown_after_quarantine() {
        use aw_agent_core::{
            Agent, AgentOptions, CompletionFuture, DeltaSink, InvocationRecord, JournalEntry,
            Message, Outcome, Provider, Turn, TurnJournal, TurnStart,
        };
        use std::sync::Arc;
        struct NeverProvider;
        impl Provider for NeverProvider {
            fn stream<'a>(
                &'a self,
                _messages: &'a [Message],
                _model: &'a str,
                _sink: &'a mut DeltaSink<'_>,
            ) -> CompletionFuture<'a> {
                Box::pin(async { panic!("no paid provider may run during cold restore") })
            }
        }
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&state).unwrap();
        let agent = Agent::builder(Arc::new(NeverProvider), AgentOptions::new("fixture", "m"))
            .build()
            .unwrap();
        let actor = agent.actor().clone();
        let identity = agent.default_identity().unwrap();
        let (session, branch) = owner
            .create(&actor, "original unknown provider bill", binding())
            .unwrap();
        let (_, journal) = owner.activate(&actor, &session.id, &branch.id).unwrap();
        journal
            .begin(TurnStart {
                turn_id: 1,
                prompt: "uncertain original provider".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        journal
            .append(
                1,
                JournalEntry::InvocationStarted {
                    ordinal: 1,
                    identity: identity.clone(),
                },
            )
            .await
            .unwrap();
        let receipt = InvocationRecord {
            ordinal: 1,
            identity: identity.clone(),
            meta: None,
            outcome: "failed_or_cancelled".into(),
            usage_verified: false,
        };
        journal
            .append(
                1,
                JournalEntry::InvocationFinished {
                    record: receipt.clone(),
                },
            )
            .await
            .unwrap();
        journal
            .finish(Turn {
                id: 1,
                prompt: "uncertain original provider".into(),
                answer: String::new(),
                model: identity.model.clone(),
                profile: identity.account_id.clone(),
                outcome: Outcome::Failed,
                meta: None,
                error: Some("unknown original bill; inspection required".into()),
                actor: Some(identity.actor.clone()),
                tool_records: vec![],
                invocations: vec![receipt],
                context_messages: vec![],
                needs_reconciliation: true,
            })
            .await
            .unwrap();
        let initial: i64 = owner
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM invocation_receipts WHERE state='unknown'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(initial, 1);
        let bundle = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-original-unknown-paid", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-unknown-paid-no-replay");
        restore_complete_cold_quarantined(&bundle, &target, |_, _| Ok(())).unwrap();
        let c = Connection::open_with_flags(
            target.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let unknown: i64 = c
            .query_row(
                "SELECT count(*) FROM invocation_receipts WHERE state='unknown'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unknown, initial);
        let provider_status: String = c
            .query_row(
                "SELECT dispatch_state FROM llm_attempts ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provider_status, "unknown");
        let review =
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&bundle, &target)
                .unwrap();
        assert_eq!(review.uncertain_provider_receipts, 1);
        assert_eq!(review.possibly_paid_attempts, 1);
        assert!(review.outstanding_review && !review.activation_authorized);
        assert!(
            crate::cold_activation_consent::issue_cold_activation_device_challenge(
                &bundle,
                &target,
                "dom_any",
                "unknown-paid-cannot-consent",
            )
            .is_err(),
            "real original uncertain provider billing blocks U consent challenge"
        );
        let new_paid: i64 = c
            .query_row(
                "SELECT count(*) FROM invocation_receipts WHERE ordinal > 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new_paid, 0);
        assert!(SessionStore::open(&target).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_original_claimed_queue_requires_inspection_not_replay_after_cold_restore() {
        // A genuinely claimed original Actor Input is NOT simply Pending:
        // crash/failover must preserve the immutable claimed fact and never
        // turn it back into a dispatchable Input on restore.
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-private-parent");
        let owner = SessionStore::open(&state).unwrap();
        let actor = PrincipalId::new("local").unwrap();
        let (session, branch) = owner
            .create(&actor, "claimed-but-uncertain-input", binding())
            .unwrap();
        let epoch = owner
            .workspace_writer(&actor, None, &session.workspace_id)
            .unwrap()
            .writer_epoch;
        let original = owner
            .actor_queue_enqueue(
                &actor,
                crate::ActorQueueRequest {
                    request_id: "original_claimed_before_cold_restore".into(),
                    session_id: session.id.clone(),
                    branch_id: branch.id.clone(),
                    recipient_site: "local".into(),
                    sender: "r9g1a1g1".into(),
                    message_id: 1,
                    writer_epoch: epoch,
                    target: crate::QueueTarget::Auto,
                    priority: 0,
                },
                "ORIGINAL_CLAIMED_INPUT_DO_NOT_AUTO_REPLAY",
            )
            .unwrap();
        let claim = owner
            .actor_queue_claim_next(&actor, &session.id, &branch.id, "local", "r9g1a1g1", epoch)
            .unwrap()
            .expect("real original Pending was claimed");
        assert_eq!(claim.fact.state, crate::QueueState::Claimed);
        assert_eq!(claim.fact.payload_ref, original.payload_ref);
        let original_claim = claim.fact.claim.as_ref().unwrap().clone();
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-preserved-claimed-hazard", |_, _| {
                Ok(())
            })
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-claimed-input-inspect");
        restore_complete_cold_quarantined(&published, &target, |_, _| Ok(())).unwrap();
        let c = Connection::open_with_flags(
            target.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let json: String = c.query_row(
            "SELECT data_json FROM audit WHERE action='actor.queue' AND session_id=?1 ORDER BY seq DESC LIMIT 1",
            [&session.id], |row| row.get(0),
        ).unwrap();
        let latest: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            latest["state"], "claimed",
            "restoration must not rewrite uncertain claimed Input back to Pending"
        );
        assert_eq!(latest["claim"]["writer_epoch"], epoch);
        let review =
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&published, &target)
                .unwrap();
        assert_eq!(review.queued_claimed, 1);
        assert_eq!(review.queued_pending, 0);
        let w7 = crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot(
            &published, &target,
        )
        .unwrap();
        assert!(!w7.preparation.runnable);
        let inspected_claim = w7
            .actor
            .iter()
            .find_map(|e| {
                if let crate::cold_effect_reconciliation::OriginalActorEvidenceSource::Queue {
                    recipient_site,
                    claim: Some(c),
                    ..
                } = &e.provenance
                {
                    assert_eq!(recipient_site, "local");
                    Some(c)
                } else {
                    None
                }
            })
            .expect("original claimed Queue Process evidence survives cold restore");
        assert_eq!(
            inspected_claim.recipient_actor,
            original_claim.recipient_actor
        );
        assert_eq!(
            inspected_claim.owner_process_id,
            original_claim.owner_process_id.unwrap()
        );
        assert_eq!(
            inspected_claim.owner_store_generation,
            original_claim.owner_store_generation.unwrap()
        );
        assert_eq!(inspected_claim.writer_epoch, epoch);
        assert_eq!(inspected_claim.turn_id, original_claim.turn_id);
        assert!(review.outstanding_review && !review.activation_authorized);
        assert!(
            SessionStore::open(&target).is_err(),
            "original queued Input has no implicit source/process reauthorization"
        );
        let vault = crate::LocalSecretStore::open(&target).unwrap();
        assert_eq!(
            vault.get(&original.payload_ref).unwrap().as_slice(),
            b"ORIGINAL_CLAIMED_INPUT_DO_NOT_AUTO_REPLAY"
        );
        // Even bypassing quarantine in a TEST COPY would meet read-only
        // writer_epoch and cannot claim this old item under epoch1.
        let still_active: i64 = c
            .query_row(
                "SELECT count(*) FROM workspace_writer_state WHERE state='active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(still_active, 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_rehashed_wrong_archive_master_and_incomplete_stage_fail_before_target_creation() {
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("published-bundles");
        let parent = root.child("target-parent");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let stage = capture_partial(&owner, &bundles, |_, _| Ok(())).unwrap();
        let incomplete = stage.directory.clone();
        let denied = parent.join("aw-restored-must-stay-absent");
        assert!(restore_complete_cold_quarantined(&incomplete, &denied, |_, _| Ok(())).is_err());
        assert!(!denied.exists());
        let published = stage
            .publish_complete(&owner, "aw-composite-staged-original", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let key = published.join("secrets/master.key");
        let original = fs::read(&key).unwrap();
        let wrong = [0x5au8; 32];
        assert_ne!(original.as_slice(), wrong);
        fs::write(&key, wrong).unwrap();
        let path = published.join("manifest.json");
        let old_manifest = fs::read(&path).unwrap();
        let mut content: CompleteManifest = serde_json::from_slice(&old_manifest).unwrap();
        content
            .files
            .iter_mut()
            .find(|f| f.path == "secrets/master.key")
            .unwrap()
            .sha256 = format!("{:x}", Sha256::digest(wrong));
        fs::write(&path, serde_json::to_vec(&content).unwrap()).unwrap();
        assert!(restore_complete_cold_quarantined(&published, &denied, |_, _| Ok(())).is_err());
        assert!(
            !denied.exists(),
            "valid hashes under wrong AEAD key never create a State"
        );
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_quarantine_marker_and_extra_files_are_not_authorized_state() {
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("published-bundles");
        let parent = root.child("target-parent");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-marker-proof", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let original = verify_complete_directory(&published).unwrap();
        let target = parent.join("aw-restored-marked-not-runnable");
        restore_complete_cold_quarantined(&published, &target, |_, _| Ok(())).unwrap();
        let gen = original.restore_generation + 1;
        verify_cold_quarantined_state(&target, &original, gen).unwrap();
        let marker = target.join(COLD_QUARANTINE_MARKER);
        fs::write(&marker, b"CLAIMED-ACTIVATED-WITHOUT-ORIGINAL-OPERATOR").unwrap();
        assert!(verify_cold_quarantined_state(&target, &original, gen).is_err());
        assert!(
            SessionStore::open(&target).is_err(),
            "a modified marker never authorizes opening"
        );
        fs::write(
            &marker,
            cold_marker(&original.store_id, original.restore_generation, gen),
        )
        .unwrap();
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
        verify_cold_quarantined_state(&target, &original, gen).unwrap();
        let injected = target.join("unverified-owner-executable");
        fs::write(&injected, b"NO_EXECUTION_FROM_COPIED_ARCHIVE").unwrap();
        fs::set_permissions(&injected, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            verify_cold_quarantined_state(&target, &original, gen).is_err(),
            "extra data in an otherwise private cold destination is not in the original manifest"
        );
        fs::remove_file(&injected).unwrap();
        verify_cold_quarantined_state(&target, &original, gen).unwrap();
    }

    /// TEST-ONLY original trusted imported device fixture. Real production
    /// bindings are created only by the original signed continuity import;
    /// this bypasses import ceremony in a private local SQLite fixture but
    /// exercises the actual same cryptographic signature/epoch verification.
    #[cfg(target_os = "linux")]
    fn aw_p6_10u_fixture_device_binding(
        owner: &SessionStore,
        principal: &PrincipalId,
    ) -> (String, String, ed25519_dalek::SigningKey) {
        let domain = owner.default_domain(principal).unwrap();
        let user = owner.current_user(principal).unwrap();
        let seed = [0x6Bu8; 32];
        let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key().to_bytes();
        let key_hex = verifying
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let key = format!("device:{:x}", Sha256::digest(verifying));
        let c = owner.connection().unwrap();
        c.execute(
            "INSERT INTO identity_continuity_grants(
                grant_id,domain_id,source_node_id,status,accepted_by_user_id,revision,accepted_at)
             VALUES('icgrant_aw_p6_10u',?1,'node_signed_fixture','active',?2,1,1)",
            rusqlite::params![domain.id, user.user_id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO portable_identity_bindings(
                binding_id,domain_id,user_id,source_store_id,source_node_id,kind,binding_key,
                device_id,public_key,user_security_epoch,domain_security_epoch,
                binding_sha256,status,imported_at)
             VALUES('pib_aw_p6_10u',?1,?2,'source_store_fixture','node_signed_fixture',
                    'device_key',?3,'device_fixture',?4,?5,?6,?7,'active',1)",
            rusqlite::params![
                domain.id,
                user.user_id,
                key,
                key_hex,
                user.security_epoch,
                1,
                "c".repeat(64)
            ],
        )
        .unwrap();
        (domain.id, key_hex, signing)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10u_real_trusted_device_signature_records_nonrunnable_consent_once() {
        use crate::cold_activation_consent::{
            issue_cold_activation_device_challenge, record_signed_cold_activation_consent,
        };
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use ed25519_dalek::Signer;
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        owner
            .create(&principal, "original low-risk owner review", binding())
            .unwrap();
        let (domain, public, signer) = aw_p6_10u_fixture_device_binding(&owner, &principal);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-u-device-consent", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        fs::remove_dir_all(&source).unwrap();
        let target = parent.join("aw-restored-u-signed-owner-review");
        restore_complete_cold_quarantined(&published, &target, |_, _| Ok(())).unwrap();
        let report = record_quarantine_review_request(
            &published,
            &target,
            &principal,
            "signed-review-one",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        assert!(!report.outstanding_review && !report.activation_authorized);
        let challenge = issue_cold_activation_device_challenge(
            &published,
            &target,
            &domain,
            "signed-review-one",
        )
        .unwrap();
        assert!(challenge
            .transcript
            .contains("aw.cold.activation.device.consent.v1"));
        let signature = STANDARD.encode(signer.sign(challenge.transcript.as_bytes()).to_bytes());
        record_signed_cold_activation_consent(&published, &target, &challenge, &public, &signature)
            .unwrap();
        assert!(
            record_signed_cold_activation_consent(
                &published, &target, &challenge, &public, &signature,
            )
            .is_err(),
            "a captured signed transcript can never be replayed within this State"
        );
        assert!(
            issue_cold_activation_device_challenge(
                &published,
                &target,
                &domain,
                "signed-review-one",
            )
            .is_err(),
            "challenge request identity must be unique"
        );
        let c = Connection::open_with_flags(
            target.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let consent: String = c
            .query_row(
                "SELECT payload_json FROM local_coordination_events
             WHERE kind='store.cold_activation_device_consent_verified' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let event: serde_json::Value = serde_json::from_str(&consent).unwrap();
        assert_eq!(event["activation_authorized"], false);
        assert_eq!(event["runnable"], false);
        assert_eq!(event["replay_paid_tool_queue"], false);
        assert!(
            !consent.contains(&signature),
            "private signature must not be stored in plaintext consent event"
        );
        assert!(target.join(COLD_QUARANTINE_MARKER).exists());
        let sql_marker: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='cold_restore_quarantine'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sql_marker.starts_with("aw.cold.quarantined.v1:"));
        let still_live: i64 = c
            .query_row(
                "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(still_live, 0);
        assert!(SessionStore::open(&target).is_err());
        assert!(
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&published, &target,)
                .is_ok()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10u_wrong_signature_revoked_device_and_replayed_challenge_fail_closed() {
        use crate::cold_activation_consent::{
            issue_cold_activation_device_challenge, record_signed_cold_activation_consent,
        };
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use ed25519_dalek::Signer;
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        owner
            .create(&principal, "original device binding", binding())
            .unwrap();
        let (domain, public, signer) = aw_p6_10u_fixture_device_binding(&owner, &principal);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-u-replay-source", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-u-signature-validation");
        restore_complete_cold_quarantined(&archive, &target, |_, _| Ok(())).unwrap();
        record_quarantine_review_request(
            &archive,
            &target,
            &principal,
            "u-signature-test",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        let challenge =
            issue_cold_activation_device_challenge(&archive, &target, &domain, "u-signature-test")
                .unwrap();
        let wrong_signer = ed25519_dalek::SigningKey::from_bytes(&[0x19u8; 32]);
        let forged = STANDARD.encode(
            wrong_signer
                .sign(challenge.transcript.as_bytes())
                .to_bytes(),
        );
        assert!(
            record_signed_cold_activation_consent(&archive, &target, &challenge, &public, &forged,)
                .is_err(),
            "a copied arbitrary device cannot sign for the original trusted owner"
        );
        let mut modified = challenge.clone();
        modified.transcript = modified.transcript.replace(
            "approve_future_separately_gated_activation_no_replay",
            "execute_every_queued_input_now",
        );
        let wrong_decision =
            STANDARD.encode(signer.sign(modified.transcript.as_bytes()).to_bytes());
        assert!(
            record_signed_cold_activation_consent(
                &archive,
                &target,
                &modified,
                &public,
                &wrong_decision,
            )
            .is_err(),
            "a changed signed decision is not the original challenge"
        );
        let valid = STANDARD.encode(signer.sign(challenge.transcript.as_bytes()).to_bytes());
        let c = Connection::open(target.join("sessions.sqlite3")).unwrap();
        c.execute("UPDATE portable_identity_bindings SET status='revoked' WHERE binding_id='pib_aw_p6_10u'",[])
            .unwrap();
        drop(c);
        assert!(
            record_signed_cold_activation_consent(&archive, &target, &challenge, &public, &valid,)
                .is_err(),
            "a previously trusted but revoked device cannot authorize a copied Store"
        );
        let c = Connection::open(target.join("sessions.sqlite3")).unwrap();
        c.execute("UPDATE portable_identity_bindings SET status='active' WHERE binding_id='pib_aw_p6_10u'",[])
            .unwrap();
        drop(c);
        let c = Connection::open(target.join("sessions.sqlite3")).unwrap();
        c.execute("UPDATE identity_continuity_grants SET status='revoked' WHERE grant_id='icgrant_aw_p6_10u'",[])
            .unwrap();
        drop(c);
        assert!(
            record_signed_cold_activation_consent(&archive, &target, &challenge, &public, &valid,)
                .is_err(),
            "revoked original source-node identity grant rejects signed consent"
        );
        let c = Connection::open(target.join("sessions.sqlite3")).unwrap();
        c.execute("UPDATE identity_continuity_grants SET status='active' WHERE grant_id='icgrant_aw_p6_10u'",[])
            .unwrap();
        c.execute(
            "UPDATE domains SET security_epoch=security_epoch+1 WHERE domain_id=?1",
            [&domain],
        )
        .unwrap();
        drop(c);
        assert!(
            record_signed_cold_activation_consent(&archive, &target, &challenge, &public, &valid,)
                .is_err(),
            "stale copied original domain security epoch invalidates device proof"
        );
        let c = Connection::open(target.join("sessions.sqlite3")).unwrap();
        c.execute(
            "UPDATE domains SET security_epoch=security_epoch-1 WHERE domain_id=?1",
            [&domain],
        )
        .unwrap();
        drop(c);
        let other = parent.join("aw-restored-u-distinct-owner-incarnation");
        restore_complete_cold_quarantined(&archive, &other, |_, _| Ok(())).unwrap();
        record_quarantine_review_request(
            &archive,
            &other,
            &principal,
            "u-signature-test",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        assert!(
            record_signed_cold_activation_consent(&archive, &other, &challenge, &public, &valid,)
                .is_err(),
            "another independently restored State did not issue this OS-random challenge"
        );
        assert!(SessionStore::open(&target).is_err());
        assert!(SessionStore::open(&other).is_err());
        // The original real active key can still sign only its exact, once-
        // issued challenge in the FIRST matching restored Store.
        record_signed_cold_activation_consent(&archive, &target, &challenge, &public, &valid)
            .unwrap();
        assert!(
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, &target,)
                .is_ok()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10u_unresolved_queue_risk_refuses_signed_consent_challenge() {
        use crate::cold_activation_consent::issue_cold_activation_device_challenge;
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        create_encrypted_queue(&owner); // a real original Pending encrypted Input
        let (domain, _, _) = aw_p6_10u_fixture_device_binding(&owner, &principal);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-u-pending-effect", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-u-risk-blocked");
        restore_complete_cold_quarantined(&archive, &target, |_, _| Ok(())).unwrap();
        record_quarantine_review_request(
            &archive,
            &target,
            &principal,
            "u-risky-pending",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        assert!(
            issue_cold_activation_device_challenge(&archive, &target, &domain, "u-risky-pending",)
                .is_err(),
            "even a trusted real device may not consent to automatic old Pending input replay"
        );
        assert!(SessionStore::open(&target).is_err());
    }

    /// Test-only fully original R/S/T/U signed consent fixture. This makes a
    /// source-independent cold State with a real original device-key signature
    /// and prior T owner review. The ephemeral test signing key never enters
    /// the copied SQLite vault, audit or any production credential store.
    #[cfg(target_os = "linux")]
    struct VFixture {
        _root: TempRoot,
        archive: PathBuf,
        restored: PathBuf,
        challenge: crate::cold_activation_consent::ColdDeviceChallenge,
        public: String,
        signature: String,
    }
    #[cfg(target_os = "linux")]
    fn aw_p6_10v_fixture() -> VFixture {
        aw_p6_10v_fixture_with_consent(true)
    }
    #[cfg(target_os = "linux")]
    fn aw_p6_10v_fixture_with_consent(record_consent: bool) -> VFixture {
        use crate::cold_activation_consent::{
            issue_cold_activation_device_challenge, record_signed_cold_activation_consent,
        };
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use ed25519_dalek::Signer;
        let root = TempRoot::new();
        let source = root.child("original-source");
        let bundles = root.child("private-archives");
        let parent = root.child("restored-owner-parent");
        let store = SessionStore::open(&source).unwrap();
        let original = PrincipalId::new("local").unwrap();
        let (session, _branch) = store
            .create(&original, "genuine original Session owner", binding())
            .unwrap();
        let _original_epoch = store
            .workspace_writer(&original, None, &session.workspace_id)
            .unwrap();
        let (domain, public, signer) = aw_p6_10u_fixture_device_binding(&store, &original);
        let archive = capture_partial(&store, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&store, "aw-composite-v-cold-approval", |_, _| Ok(()))
            .unwrap();
        drop(store);
        let restored = parent.join("aw-restored-v-nonrunnable-state");
        restore_complete_cold_quarantined(&archive, &restored, |_, _| Ok(())).unwrap();
        record_quarantine_review_request(
            &archive,
            &restored,
            &original,
            "v-fully-signed",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        let challenge =
            issue_cold_activation_device_challenge(&archive, &restored, &domain, "v-fully-signed")
                .unwrap();
        let signature = STANDARD.encode(signer.sign(challenge.transcript.as_bytes()).to_bytes());
        if record_consent {
            record_signed_cold_activation_consent(
                &archive, &restored, &challenge, &public, &signature,
            )
            .unwrap();
        }
        VFixture {
            _root: root,
            archive,
            restored,
            challenge,
            public,
            signature,
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_revocation_after_original_signed_consent_denies_dry_run() {
        use crate::cold_activation_dry_run::inspect_cold_activation_dry_run;
        let fixture = aw_p6_10v_fixture();
        let verify = || {
            inspect_cold_activation_dry_run(
                &fixture.archive,
                &fixture.restored,
                &fixture.challenge,
                &fixture.public,
                &fixture.signature,
            )
        };
        assert!(verify().is_ok());
        let db = fixture.restored.join("sessions.sqlite3");
        let c = Connection::open(&db).unwrap();
        c.execute("UPDATE portable_identity_bindings SET status='revoked' WHERE binding_id='pib_aw_p6_10u'",[]).unwrap();
        drop(c);
        assert!(
            verify().is_err(),
            "copied U receipt with revoked trusted device cannot be independently dry-run verified"
        );
        let c = Connection::open(&db).unwrap();
        c.execute("UPDATE portable_identity_bindings SET status='active' WHERE binding_id='pib_aw_p6_10u'",[]).unwrap();
        c.execute("UPDATE identity_continuity_grants SET status='revoked' WHERE grant_id='icgrant_aw_p6_10u'",[]).unwrap();
        drop(c);
        assert!(
            verify().is_err(),
            "revoking the original source-node trust after U consent must invalidate V"
        );
        let c = Connection::open(&db).unwrap();
        c.execute("UPDATE identity_continuity_grants SET status='active' WHERE grant_id='icgrant_aw_p6_10u'",[]).unwrap();
        c.execute("UPDATE users SET security_epoch=security_epoch+1 WHERE user_id=(SELECT user_id FROM portable_identity_bindings WHERE binding_id='pib_aw_p6_10u')",[]).unwrap();
        drop(c);
        assert!(
            verify().is_err(),
            "original user's security epoch advance invalidates prior device approval"
        );
        assert!(SessionStore::open(&fixture.restored).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_stale_writer_epoch_after_signed_consent_blocks_dry_run() {
        use crate::cold_activation_dry_run::inspect_cold_activation_dry_run;
        let fixture = aw_p6_10v_fixture();
        let proof = || {
            inspect_cold_activation_dry_run(
                &fixture.archive,
                &fixture.restored,
                &fixture.challenge,
                &fixture.public,
                &fixture.signature,
            )
        };
        let inspected = proof().unwrap();
        assert!(
            inspected.would_require_fresh_writer_epochs > 0,
            "real original Workspace writer must require fresh authority, not copied WriterEpoch"
        );
        let c = Connection::open(fixture.restored.join("sessions.sqlite3")).unwrap();
        c.execute("UPDATE workspace_writer_state SET state='active'", [])
            .unwrap();
        drop(c);
        assert!(proof().is_err(),
            "even a real U signature cannot dry-run when restored previous WriterEpoch becomes active");
        let c = Connection::open(fixture.restored.join("sessions.sqlite3")).unwrap();
        c.execute("UPDATE workspace_writer_state SET state='read_only'", [])
            .unwrap();
        drop(c);
        assert!(proof().is_ok());
        assert!(SessionStore::open(&fixture.restored).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_copied_restored_state_never_becomes_runnable_authority() {
        use crate::cold_activation_dry_run::inspect_cold_activation_dry_run;
        use std::os::unix::fs::PermissionsExt;
        // A full same-UID copy of SQLite, marker and device receipt cannot be
        // distinguished by purely local v33 metadata. U/V correctly NEVER
        // treat it as runnable; cross-host uniqueness requires an external
        // independent owner nonce or signed authority not yet implemented.
        fn copy_owned_tree(src: &Path, dst: &Path) {
            fs::create_dir(dst).unwrap();
            fs::set_permissions(dst, fs::Permissions::from_mode(0o700)).unwrap();
            for entry in fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let from = entry.path();
                let to = dst.join(entry.file_name());
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    copy_owned_tree(&from, &to);
                } else {
                    assert!(meta.is_file());
                    fs::copy(&from, &to).unwrap();
                    fs::set_permissions(&to, fs::Permissions::from_mode(0o600)).unwrap();
                }
            }
        }
        let fixture = aw_p6_10v_fixture();
        let duplicated = fixture._root.0.join("aw-restored-v-copied-quarantine");
        copy_owned_tree(&fixture.restored, &duplicated);
        // Even if this local clone has a perfectly matching copied original
        // U immutable receipt and valid provided device signature, it cannot
        // grant executable activation or open an original owner Process.
        let clone_review = inspect_cold_activation_dry_run(
            &fixture.archive,
            &duplicated,
            &fixture.challenge,
            &fixture.public,
            &fixture.signature,
        )
        .unwrap();
        assert!(clone_review.consent_evidence_reverified);
        assert!(clone_review.requires_fresh_human_origin);
        assert!(!clone_review.activation_authorized && !clone_review.runnable);
        assert!(SessionStore::open(&duplicated).is_err());
        assert!(SessionStore::open(&fixture.restored).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_new_unfinished_original_effect_after_consent_blocks_dry_run() {
        use crate::cold_activation_dry_run::inspect_cold_activation_dry_run;
        let fixture = aw_p6_10v_fixture();
        let verify = || {
            inspect_cold_activation_dry_run(
                &fixture.archive,
                &fixture.restored,
                &fixture.challenge,
                &fixture.public,
                &fixture.signature,
            )
        };
        assert!(verify().is_ok());
        let db = fixture.restored.join("sessions.sqlite3");
        // TEST-ONLY negative control: synthetic new unfinished original Run
        // added after real signed U consent. The original immutable Queue,
        // Effect and Provider tools are NEVER called or automatically retried.
        // This simulates a trusted-local stale mutation between signing and
        // dry-run, not normal operation (the cold Store stays quarantined).
        let c = Connection::open(&db).unwrap();
        let created = c
            .execute(
                "INSERT INTO runs(id,session_id,branch_id,turn_id,prompt_node,identity_json,
                               root_key,status,possible_effect,started_at)
             SELECT 'run_v_unfinished',s.id,b.id,987654,b.head,'{}',
                    'run_v_unique_root','running',1,0
             FROM sessions s JOIN branches b ON b.session_id=s.id LIMIT 1",
                [],
            )
            .unwrap();
        assert_eq!(created, 1);
        drop(c);
        assert!(verify().is_err(),
            "a new original unfinished, potentially effectful Run after consent invalidates the copied risk fingerprint");
        let c = Connection::open(&db).unwrap();
        c.execute("DELETE FROM runs WHERE id='run_v_unfinished'", [])
            .unwrap();
        drop(c);
        assert!(verify().is_ok(),
            "an absent effect means a new READ-ONLY dry run may inspect again; never activate automatically");
        assert!(SessionStore::open(&fixture.restored).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_a_new_effect_between_signature_scan_and_audit_cas_must_be_denied() {
        use crate::cold_activation_dry_run::{
            inspect_cold_activation_dry_run, record_cold_activation_dry_run_with_pretransaction,
        };
        let fixture = aw_p6_10v_fixture();
        assert!(inspect_cold_activation_dry_run(
            &fixture.archive,
            &fixture.restored,
            &fixture.challenge,
            &fixture.public,
            &fixture.signature,
        )
        .is_ok());
        let result = record_cold_activation_dry_run_with_pretransaction(
            &fixture.archive,
            &fixture.restored,
            &fixture.challenge,
            &fixture.public,
            &fixture.signature,
            "inflight-effect-cas",
            |root| {
                // Test-only original-v33 SQL injection at the exact seam
                // AFTER the first valid U signature/risk scan and BEFORE
                // the audit transaction's BEGIN IMMEDIATE. No model/Tool
                // executor or external paid provider is invoked.
                let db = crate::db(Connection::open(root.join("sessions.sqlite3")))?;
                let inserted = crate::db(db.execute(
                    "INSERT INTO runs(id,session_id,branch_id,turn_id,prompt_node,identity_json,
                                       root_key,status,possible_effect,started_at)
                     SELECT 'run_v_after_scan',s.id,b.id,987659,b.head,'{}',
                            'run_v_new_effect_root','running',1,0
                     FROM sessions s JOIN branches b ON b.session_id=s.id LIMIT 1",
                    [],
                ))?;
                assert_eq!(inserted, 1);
                Ok(())
            },
        );
        assert!(result.is_err(),
            "a newly started effect between read-only consent check and SQL audit write MUST invalidate that audit");
        let c = Connection::open_with_flags(
            fixture.restored.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let false_receipts: i64 = c
            .query_row(
                "SELECT count(*) FROM local_coordination_events
             WHERE kind='store.cold_activation_dry_run_recorded'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            false_receipts, 0,
            "no immutable dry-run receipt can hide a newly unknown effect"
        );
        assert!(SessionStore::open(&fixture.restored).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_revoked_device_between_scan_and_audit_never_creates_receipt() {
        use crate::cold_activation_dry_run::{
            record_cold_activation_dry_run, record_cold_activation_dry_run_with_pretransaction,
        };
        let fixture = aw_p6_10v_fixture();
        let denied = record_cold_activation_dry_run_with_pretransaction(
            &fixture.archive,
            &fixture.restored,
            &fixture.challenge,
            &fixture.public,
            &fixture.signature,
            "revoked-inflight-device",
            |root| {
                let db = crate::db(Connection::open(root.join("sessions.sqlite3")))?;
                crate::db(db.execute(
                    "UPDATE portable_identity_bindings SET status='revoked'
                     WHERE binding_id='pib_aw_p6_10u'",
                    [],
                ))?;
                Ok(())
            },
        );
        assert!(denied.is_err(),
            "a U signature verified before source-key revocation cannot satisfy the final transactional V proof");
        let c = Connection::open(fixture.restored.join("sessions.sqlite3")).unwrap();
        let invalid_count: i64 = c
            .query_row(
                "SELECT count(*) FROM local_coordination_events
             WHERE kind='store.cold_activation_dry_run_recorded'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(invalid_count, 0);
        c.execute(
            "UPDATE portable_identity_bindings SET status='active'
             WHERE binding_id='pib_aw_p6_10u'",
            [],
        )
        .unwrap();
        drop(c);
        let verified = record_cold_activation_dry_run(
            &fixture.archive,
            &fixture.restored,
            &fixture.challenge,
            &fixture.public,
            &fixture.signature,
            "revoked-inflight-device",
        )
        .unwrap();
        assert!(!verified.activation_authorized && !verified.runnable);
        assert!(verified.requires_fresh_human_origin);
        assert!(SessionStore::open(&fixture.restored).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_competing_native_processes_record_only_one_no_effect_dry_run() {
        use crate::cold_activation_consent::ColdDeviceChallenge;
        use crate::cold_activation_dry_run::record_cold_activation_dry_run;
        const ARCHIVE: &str = "AW_P6_10V_CHILD_ARCHIVE";
        const STATE: &str = "AW_P6_10V_CHILD_STATE";
        const TRANSCRIPT: &str = "AW_P6_10V_CHILD_TRANSCRIPT";
        const PUBKEY: &str = "AW_P6_10V_CHILD_PUBKEY";
        const SIGNATURE: &str = "AW_P6_10V_CHILD_SIGNATURE";
        if let Some(path) = std::env::var_os(ARCHIVE) {
            let source = PathBuf::from(path);
            let cold = PathBuf::from(std::env::var_os(STATE).unwrap());
            let challenge = ColdDeviceChallenge {
                transcript: std::env::var(TRANSCRIPT).unwrap(),
            };
            let pubkey = std::env::var(PUBKEY).unwrap();
            let signature = std::env::var(SIGNATURE).unwrap();
            match record_cold_activation_dry_run(
                &source,
                &cold,
                &challenge,
                &pubkey,
                &signature,
                "same-native-race",
            ) {
                Ok(result) => {
                    assert!(!result.runnable && !result.activation_authorized);
                    std::process::exit(0);
                }
                Err(_) => std::process::exit(47),
            }
        }
        let fixture = aw_p6_10v_fixture();
        let binary = std::env::current_exe().unwrap();
        let launch = || {
            std::process::Command::new(&binary)
                .arg("--exact")
                .arg("composite_staging::tests::aw_p6_10v_competing_native_processes_record_only_one_no_effect_dry_run")
                .arg("--nocapture")
                .env(ARCHIVE,&fixture.archive)
                .env(STATE,&fixture.restored)
                .env(TRANSCRIPT,&fixture.challenge.transcript)
                .env(PUBKEY,&fixture.public)
                .env(SIGNATURE,&fixture.signature)
                .spawn().unwrap()
        };
        let mut first = launch();
        let mut second = launch();
        let mut results = [
            first.wait().unwrap().code().unwrap(),
            second.wait().unwrap().code().unwrap(),
        ];
        results.sort_unstable();
        assert_eq!(
            results,
            [0, 47],
            "one native process must atomically win original v33 immutable review request CAS"
        );
        let c = Connection::open_with_flags(
            fixture.restored.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let receipts: i64 = c
            .query_row(
                "SELECT count(*) FROM local_coordination_events
             WHERE kind='store.cold_activation_dry_run_recorded'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipts, 1);
        let active: i64 = c
            .query_row(
                "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active, 0);
        drop(c);
        assert!(fixture.restored.join(COLD_QUARANTINE_MARKER).exists());
        assert!(SessionStore::open(&fixture.restored).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_signed_original_device_evidence_is_nonrunnable_even_after_dry_run() {
        use crate::cold_activation_consent::{
            issue_cold_activation_device_challenge, record_signed_cold_activation_consent,
        };
        use crate::cold_activation_dry_run::{
            inspect_cold_activation_dry_run, record_cold_activation_dry_run,
        };
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use ed25519_dalek::Signer;
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&state).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        owner
            .create(
                &principal,
                "original owner no outstanding effects",
                binding(),
            )
            .unwrap();
        let (domain, public, signer) = aw_p6_10u_fixture_device_binding(&owner, &principal);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-v-dryrun", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        fs::remove_dir_all(&state).unwrap();
        let target = parent.join("aw-restored-v-zero-effect-test");
        restore_complete_cold_quarantined(&archive, &target, |_, _| Ok(())).unwrap();
        let _ = record_quarantine_review_request(
            &archive,
            &target,
            &principal,
            "v-request",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        let challenge =
            issue_cold_activation_device_challenge(&archive, &target, &domain, "v-request")
                .unwrap();
        let signed = STANDARD.encode(signer.sign(challenge.transcript.as_bytes()).to_bytes());
        assert!(
            inspect_cold_activation_dry_run(&archive, &target, &challenge, &public, &signed,)
                .is_err(),
            "a valid key signature without the genuine recorded U consent is insufficient"
        );
        record_signed_cold_activation_consent(&archive, &target, &challenge, &public, &signed)
            .unwrap();
        let before = fs::read(target.join("sessions.sqlite3")).unwrap();
        let proof =
            inspect_cold_activation_dry_run(&archive, &target, &challenge, &public, &signed)
                .unwrap();
        assert_eq!(proof.active_writers, 0);
        assert_eq!(proof.active_processes, 0);
        assert!(proof.requires_fresh_human_origin);
        assert!(!proof.activation_authorized);
        assert!(!proof.runnable);
        assert_eq!(
            fs::read(target.join("sessions.sqlite3")).unwrap(),
            before,
            "a purely read-only dry-run must not mutate any copied SQLite data"
        );
        record_cold_activation_dry_run(
            &archive,
            &target,
            &challenge,
            &public,
            &signed,
            "v-dry-run-once",
        )
        .unwrap();
        assert!(
            record_cold_activation_dry_run(
                &archive,
                &target,
                &challenge,
                &public,
                &signed,
                "v-dry-run-once",
            )
            .is_err(),
            "the original immutable dry-run audit ID must not be reusable"
        );
        let c = Connection::open_with_flags(
            target.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let event: String = c
            .query_row(
                "SELECT payload_json FROM local_coordination_events
            WHERE kind='store.cold_activation_dry_run_recorded' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&event).unwrap();
        assert_eq!(payload["activation_authorized"], false);
        assert_eq!(payload["runnable"], false);
        assert_eq!(payload["requires_fresh_human_origin"], true);
        assert!(!event.contains(&signed));
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(c);
        assert!(target.join(COLD_QUARANTINE_MARKER).is_file());
        assert!(SessionStore::open(&target).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10v_rehashed_forged_local_consent_event_is_not_a_signature() {
        use crate::cold_activation_consent::{
            issue_cold_activation_device_challenge, ColdDeviceChallenge,
        };
        use crate::cold_activation_dry_run::inspect_cold_activation_dry_run;
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use ed25519_dalek::Signer;
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&state).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        owner
            .create(&principal, "original owner signed review", binding())
            .unwrap();
        let (domain, public, signer) = aw_p6_10u_fixture_device_binding(&owner, &principal);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-v-copied-receipt", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-v-copied-receipt");
        restore_complete_cold_quarantined(&archive, &target, |_, _| Ok(())).unwrap();
        record_quarantine_review_request(
            &archive,
            &target,
            &principal,
            "v-fake",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        let challenge =
            issue_cold_activation_device_challenge(&archive, &target, &domain, "v-fake").unwrap();
        let signature = STANDARD.encode(signer.sign(challenge.transcript.as_bytes()).to_bytes());
        let alternate = ColdDeviceChallenge {
            transcript: challenge.transcript.replace("v-fake", "other-restoration"),
        };
        assert!(inspect_cold_activation_dry_run(
            &archive, &target, &alternate, &public, &signature,
        )
        .is_err());
        let wrong = ed25519_dalek::SigningKey::from_bytes(&[0x89u8; 32]);
        let invalid = STANDARD.encode(wrong.sign(challenge.transcript.as_bytes()).to_bytes());
        assert!(
            inspect_cold_activation_dry_run(&archive, &target, &challenge, &public, &invalid,)
                .is_err()
        );
        assert!(
            inspect_cold_activation_dry_run(&archive, &target, &challenge, &public, &signature,)
                .is_err(),
            "a genuine signature still needs the actual original U immutable consent event"
        );
        assert!(SessionStore::open(&target).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10u_untrusted_local_label_cannot_issue_signed_activation_challenge() {
        use crate::cold_activation_consent::issue_cold_activation_device_challenge;
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        let principal = PrincipalId::new("local").unwrap();
        owner
            .create(&principal, "local-only has NO device binding", binding())
            .unwrap();
        let domain = owner.default_domain(&principal).unwrap().id;
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-u-no-device", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-u-no-device-binding");
        restore_complete_cold_quarantined(&archive, &target, |_, _| Ok(())).unwrap();
        record_quarantine_review_request(
            &archive,
            &target,
            &principal,
            "review-without-key",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        assert!(
            issue_cold_activation_device_challenge(
                &archive,
                &target,
                &domain,
                "review-without-key",
            )
            .is_err(),
            "a copied local Unix principal review alone is not independently signed human consent"
        );
        assert!(SessionStore::open(&target).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10t_read_only_inspection_cannot_authorize_even_empty_state() {
        let root = TempRoot::new();
        let state = root.child("source-empty");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&state).unwrap();
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-empty-review", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let destination = parent.join("aw-restored-zero-issues-not-approved");
        restore_complete_cold_quarantined(&published, &destination, |_, _| Ok(())).unwrap();
        let original = fs::read(destination.join("sessions.sqlite3")).unwrap();
        let review = crate::cold_quarantine_review::inspect_quarantined_cold_restore(
            &published,
            &destination,
        )
        .unwrap();
        assert_eq!(review.sessions, 0);
        assert!(!review.outstanding_review);
        assert!(
            !review.activation_authorized,
            "zero hazards do not constitute original initiating-human authorization"
        );
        assert_eq!(
            fs::read(destination.join("sessions.sqlite3")).unwrap(),
            original,
            "data-only inspection must not mutate original restored SQLite bytes"
        );
        assert!(SessionStore::open(&destination).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10t_pending_queue_requires_review_and_remains_unclaimed() {
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&state).unwrap();
        let original_ref = create_encrypted_queue(&owner);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-review-pending", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let destination = parent.join("aw-restored-review-pending-never-claim");
        restore_complete_cold_quarantined(&published, &destination, |_, _| Ok(())).unwrap();
        let report = crate::cold_quarantine_review::inspect_quarantined_cold_restore(
            &published,
            &destination,
        )
        .unwrap();
        assert_eq!(report.queued_pending, 1);
        assert_eq!(report.queued_claimed, 0);
        assert!(report.outstanding_review);
        assert!(!report.activation_authorized);
        let c = Connection::open_with_flags(
            destination.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let latest: String = c
            .query_row(
                "SELECT data_json FROM audit WHERE action='actor.queue' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let fact: serde_json::Value = serde_json::from_str(&latest).unwrap();
        assert_eq!(fact["state"], "pending");
        assert!(crate::LocalSecretStore::open(&destination)
            .unwrap()
            .exists(&original_ref)
            .unwrap());
        assert!(SessionStore::open(&destination).is_err());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn aw_p6_10t_unmatched_original_tool_records_cannot_be_hidden_by_other_receipts() {
        use aw_agent_core::{
            Agent, AgentOptions, CompletionFuture, DeltaSink, JournalEntry, Message, Outcome,
            Provider, ToolCall, ToolEffect, ToolRecord, ToolSourceEvidence, ToolStatus, Turn,
            TurnJournal, TurnStart,
        };
        use std::sync::Arc;
        struct UnusedProvider;
        impl Provider for UnusedProvider {
            fn stream<'a>(
                &'a self,
                _: &'a [Message],
                _: &'a str,
                _: &'a mut DeltaSink<'_>,
            ) -> CompletionFuture<'a> {
                Box::pin(async {
                    panic!("inspection must never invoke a real or synthetic provider")
                })
            }
        }
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let store = SessionStore::open(&source).unwrap();
        let agent = Agent::builder(Arc::new(UnusedProvider), AgentOptions::new("fixture", "m"))
            .build()
            .unwrap();
        let actor = agent.actor().clone();
        let identity = agent.default_identity().unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "test-only abnormal historical Tool receipts",
                binding(),
            )
            .unwrap();
        let (_, journal) = store.activate(&actor, &session.id, &branch.id).unwrap();
        journal
            .begin(TurnStart {
                turn_id: 1,
                prompt: "factual Tool provenance".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        journal
            .finish(Turn {
                id: 1,
                prompt: "factual Tool provenance".into(),
                answer: String::new(),
                model: identity.model.clone(),
                profile: identity.account_id.clone(),
                outcome: Outcome::Failed,
                meta: None,
                error: Some("original Tool provenance inspected".into()),
                actor: Some(identity.actor.clone()),
                invocations: vec![],
                tool_records: vec![],
                context_messages: vec![],
                needs_reconciliation: false,
            })
            .await
            .unwrap();
        // TEST-ONLY v33 fixture: inject two deliberately INCONSISTENT factual
        // run_events under one real finished original Run. No OS Tool runs;
        // this models an interrupted/crash-lost terminal receipt. The review
        // must correlate exact call IDs, not subtract unrelated event totals.
        let run: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT id FROM runs WHERE session_id=?1 LIMIT 1",
                [&session.id],
                |row| row.get(0),
            )
            .unwrap();
        let started = ToolRecord {
            call: ToolCall {
                id: "unsettled_read_a".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path":"synthetic-fixture"}),
            },
            effect: ToolEffect::Read,
            execution_site: "local".into(),
            status: ToolStatus::Running,
            output: None,
        };
        let finished = ToolRecord {
            call: ToolCall {
                id: "unrelated_read_b".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path":"synthetic-fixture"}),
            },
            effect: ToolEffect::Read,
            execution_site: "local".into(),
            status: ToolStatus::Succeeded,
            output: Some(aw_agent_core::ToolOutput::text("fixture")),
        };
        let immutable_events = [
            (
                "tool_started",
                JournalEntry::ToolStarted {
                    record: started,
                    source: Some(ToolSourceEvidence::trusted_host()),
                },
            ),
            (
                "tool_finished",
                JournalEntry::ToolFinished { record: finished },
            ),
        ];
        {
            let db = store.connection().unwrap();
            for (kind, event) in immutable_events {
                db.execute(
                    "INSERT INTO run_events(run_id,kind,payload,created_at) VALUES(?1,?2,?3,0)",
                    rusqlite::params![run, kind, serde_json::to_string(&event).unwrap()],
                )
                .unwrap();
            }
        }
        let archive = capture_partial(&store, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&store, "aw-composite-test-unpaired-tool", |_, _| Ok(()))
            .unwrap();
        drop(store);
        let cold = parent.join("aw-restored-tool-evidence-review");
        restore_complete_cold_quarantined(&archive, &cold, |_, _| Ok(())).unwrap();
        let inspection =
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, &cold)
                .unwrap();
        assert_eq!(inspection.incomplete_tool_intents, 1);
        assert_eq!(inspection.mismatched_tool_receipts, 1);
        // W7 must preserve even a finished-without-start original Tool
        // contradiction as an inspection obligation, never manufacture its
        // missing start or discard original SourceKind/ID/epoch/site evidence.
        let evidence = crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot(
            &archive, &cold,
        )
        .unwrap();
        assert_eq!(evidence.tools.len(), 2);
        let started = evidence
            .tools
            .iter()
            .find(|r| r.call_id == "unsettled_read_a")
            .unwrap();
        assert_eq!(
            started.original_source.as_ref(),
            Some(&ToolSourceEvidence::trusted_host())
        );
        assert_eq!(started.original_site, "local");
        assert_eq!(started.starts, 1);
        assert_eq!(started.finishes, 0);
        let orphan_finish = evidence
            .tools
            .iter()
            .find(|r| r.call_id == "unrelated_read_b")
            .unwrap();
        assert_eq!(orphan_finish.original_source, None);
        assert_eq!(orphan_finish.starts, 0);
        assert_eq!(orphan_finish.finishes, 1);
        // W8's exact source review retains this ToolStart's original Host
        // grant provenance as historical evidence, not a grant to run again.
        let selector = crate::cold_effect_reconciliation::review::ColdEffectSubject::Tool {
            kind: started.kind,
            run_id: started.run_id.clone(),
            call_id: started.call_id.clone(),
        };
        let plan = crate::cold_effect_reconciliation::review::prepare_exact_original_effect_review(
            &archive, &cold, &selector,
        )
        .unwrap();
        let owner = PrincipalId::new("local").unwrap();
        let receipt = crate::cold_effect_reconciliation::review::record_original_owner_effect_review(
            &archive, &cold, &owner, "w8-incomplete-original-tool", &plan,
            crate::cold_effect_reconciliation::review::ColdEffectReviewIntent::RequestFreshProposalReview,
        ).unwrap();
        assert!(
            !receipt.runnable
                && !receipt.original_effect_settled
                && !receipt.fresh_authority_granted
        );
        assert_eq!(
            receipt.original_evidence_sha256,
            plan.original_evidence_sha256
        );
        let after =
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, &cold)
                .unwrap();
        assert_eq!(after.incomplete_tool_intents, 1);
        assert_eq!(after.mismatched_tool_receipts, 1);
        assert!(
            !evidence.preparation.runnable && !evidence.preparation.permits_original_effect_replay
        );
        assert!(inspection.outstanding_review && !inspection.activation_authorized);
        assert!(SessionStore::open(&cold).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10t_only_original_owner_can_record_nonactivating_review_request() {
        use crate::cold_quarantine_review::{record_quarantine_review_request, ColdReviewIntent};
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        create_encrypted_queue(&owner);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-t-owner-review", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let state = parent.join("aw-restored-t-review-still-not-approved");
        restore_complete_cold_quarantined(&archive, &state, |_, _| Ok(())).unwrap();
        let rightful = PrincipalId::new("local").unwrap();
        let stranger = PrincipalId::new("unrelated-reviewer").unwrap();
        assert!(
            record_quarantine_review_request(
                &archive,
                &state,
                &stranger,
                "review-one",
                ColdReviewIntent::RequestSeparateActivationReview,
            )
            .is_err(),
            "another principal cannot silently acknowledge another Session owner's risk"
        );
        let summary = record_quarantine_review_request(
            &archive,
            &state,
            &rightful,
            "review-one",
            ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
        assert_eq!(summary.queued_pending, 1);
        assert!(summary.outstanding_review && !summary.activation_authorized);
        assert!(
            record_quarantine_review_request(
                &archive,
                &state,
                &rightful,
                "review-one",
                ColdReviewIntent::RequestSeparateActivationReview,
            )
            .is_err(),
            "an immutable review request ID is strictly once per original restore"
        );
        record_quarantine_review_request(
            &archive,
            &state,
            &rightful,
            "review-two",
            ColdReviewIntent::KeepQuarantined,
        )
        .unwrap();
        let c = Connection::open_with_flags(
            state.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let receipts: i64 = c
            .query_row(
                "SELECT count(*) FROM local_coordination_events
             WHERE kind='store.cold_quarantine_review_requested'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(receipts, 2);
        let recorded: String = c
            .query_row(
                "SELECT payload_json FROM local_coordination_events
             WHERE kind='store.cold_quarantine_review_requested' ORDER BY seq LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&recorded).unwrap();
        assert_eq!(payload["intent"], "request_separate_activation_review");
        assert_eq!(payload["activation_authorized"], false);
        assert_eq!(payload["pending_queued_inputs"], 1);
        assert!(
            !recorded.contains("SYNTHETIC"),
            "review receipts contain no original Queue bodies, provider keys or ToolOutput"
        );
        drop(c);
        assert_eq!(
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, &state,)
                .unwrap()
                .queued_pending,
            1
        );
        assert!(
            SessionStore::open(&state).is_err(),
            "an auditable original owner review request must never lift SQL/file quarantine"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10t_mismatched_sql_quarantine_is_never_an_activation_receipt() {
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        create_encrypted_queue(&owner);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-t-sql-guard", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-t-sql-marker-tamper");
        restore_complete_cold_quarantined(&archive, &target, |_, _| Ok(())).unwrap();
        let c = Connection::open(target.join("sessions.sqlite3")).unwrap();
        let correct: String = c
            .query_row(
                "SELECT value FROM store_meta WHERE key='cold_restore_quarantine'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        c.execute(
            "UPDATE store_meta SET value='operator_checked_fake_approval' WHERE key='cold_restore_quarantine'",
            [],
        ).unwrap();
        drop(c);
        assert!(
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, &target,)
                .is_err(),
            "invalid SQL marker cannot masquerade as reviewed activation"
        );
        assert!(SessionStore::open(&target).is_err());
        let c = Connection::open(target.join("sessions.sqlite3")).unwrap();
        c.execute(
            "UPDATE store_meta SET value=?1 WHERE key='cold_restore_quarantine'",
            [&correct],
        )
        .unwrap();
        drop(c);
        let reviewed =
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, &target)
                .unwrap();
        assert_eq!(reviewed.queued_pending, 1);
        assert!(!reviewed.activation_authorized);
        assert!(SessionStore::open(&target).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10t_removing_file_marker_does_not_create_new_owner_or_activate_store() {
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("cold-target-parent");
        let owner = SessionStore::open(&source).unwrap();
        create_encrypted_queue(&owner);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-t-quarantine", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let destination = parent.join("aw-restored-t-missing-file-marker");
        restore_complete_cold_quarantined(&published, &destination, |_, _| Ok(())).unwrap();
        // Negative-first: the old S implementation only tested THIS marker.
        // Removing a local file must NOT grant permission to run the copied
        // Agent or register a fresh process from a restored paid/Tool history.
        let marker = destination.join(COLD_QUARANTINE_MARKER);
        fs::remove_file(&marker).unwrap();
        assert!(
            SessionStore::open(&destination).is_err(),
            "removing only the OS marker cannot grant owner Process authority"
        );
        assert!(
            !destination.join("owner.lock").exists(),
            "cold quarantine admission must fail before even creating owner.lock"
        );
        let c = Connection::open_with_flags(
            destination.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let live: i64 = c
            .query_row(
                "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            live, 0,
            "a failed cold activation cannot register any process"
        );
        assert!(SessionStore::open_trusted_local_direct(
            &destination,
            crate::TrustedLocalDirectOptions {
                heartbeat_ttl: Duration::from_secs(6)
            },
        )
        .is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_empty_original_vault_restores_quarantined_without_new_master() {
        let root = TempRoot::new();
        let source = root.child("source-empty-vault");
        let bundles = root.child("bundles");
        let parent = root.child("target-parent");
        let owner = SessionStore::open(&source).unwrap();
        assert!(!source.join("secrets/master.key").exists());
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-never-used-vault", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-no-invented-master-key");
        restore_complete_cold_quarantined(&archive, &target, |_, _| Ok(())).unwrap();
        assert!(
            !target.join("secrets/master.key").exists(),
            "cold restore must not generate an unrelated empty-vault master key"
        );
        assert!(target.join(COLD_QUARANTINE_MARKER).is_file());
        assert!(SessionStore::open(&target).is_err());
        let c = Connection::open_with_flags(
            target.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let active: i64 = c
            .query_row(
                "SELECT count(*) FROM local_process_incarnations WHERE state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active, 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_tampered_staged_ciphertext_during_restore_never_publishes() {
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("target-parent");
        let owner = SessionStore::open(&source).unwrap();
        let item = create_encrypted_queue(&owner);
        let archive = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-copy-time-tamper", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = parent.join("aw-restored-reject-copy-time-tamper");
        let mut mutated = false;
        let outcome = restore_complete_cold_quarantined(&archive, &target, |step, staged| {
            if step == ColdRestoreStep::AfterCopiedFile(3) {
                let item_path = staged.join(format!("secrets/items/{item}.bin"));
                let mut bytes = fs::read(&item_path).unwrap();
                *bytes.last_mut().unwrap() ^= 0x40;
                fs::write(&item_path, bytes).unwrap();
                fs::set_permissions(&item_path, fs::Permissions::from_mode(0o600)).unwrap();
                mutated = true;
            }
            Ok(())
        });
        assert!(
            mutated && outcome.is_err(),
            "original Stage AEAD + manifest copy receipts must reject a tamper after copying"
        );
        assert!(!target.exists());
        assert_eq!(
            fs::read_dir(&parent).unwrap().count(),
            0,
            "tamper cannot publish or leak a partial State directory"
        );
        assert!(verify_complete_directory(&archive).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_invalid_or_missing_composite_never_creates_restored_state() {
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let target_parent = root.child("destination-parent");
        let owner = SessionStore::open(&source).unwrap();
        let original = create_encrypted_queue(&owner);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-protected", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let target = target_parent.join("aw-restored-denied-source");
        let blob = published.join(format!("secrets/items/{original}.bin"));
        let pristine = fs::read(&blob).unwrap();
        let mut corrupt = pristine.clone();
        *corrupt.last_mut().unwrap() ^= 0x50;
        fs::write(&blob, &corrupt).unwrap();
        assert!(restore_complete_cold_quarantined(&published, &target, |_, _| Ok(())).is_err());
        assert!(!target.exists());
        fs::write(&blob, pristine).unwrap();
        assert!(restore_complete_cold_quarantined(&published, &target, |_, _| Ok(())).is_ok());
        assert!(target.join(".aw-cold-restore-pending").exists());
        assert!(
            restore_complete_cold_quarantined(&published, &target, |_, _| Ok(())).is_err(),
            "an already restored State path must never be overwritten"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_failure_after_every_copied_file_and_after_offline_fence_cleans_only_stage() {
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("destination-parent");
        let owner = SessionStore::open(&source).unwrap();
        create_encrypted_queue(&owner);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-fault-matrix", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let prior = parent.join("preexisting-cold-state-backup");
        fs::write(&prior, b"PRESERVE_COLD_RESTORE_OWNER_OLD_BYTES").unwrap();
        fs::set_permissions(&prior, fs::Permissions::from_mode(0o600)).unwrap();
        for failure in [
            ColdRestoreStep::AfterCopiedFile(1),
            ColdRestoreStep::AfterCopiedFile(2),
            ColdRestoreStep::AfterCopiedFile(3),
            ColdRestoreStep::AfterOfflineFence,
            ColdRestoreStep::BeforeRename,
        ] {
            let dest = parent.join("aw-restored-not-published");
            let mut seen = false;
            let result = restore_complete_cold_quarantined(&published, &dest, |step, _| {
                if step == failure {
                    seen = true;
                    return Err(Error::Denied(
                        "synthetic internal cold-restore interruption",
                    ));
                }
                Ok(())
            });
            assert!(
                seen && result.is_err(),
                "each real source/SQL/fence stage must be injected"
            );
            assert!(
                !dest.exists(),
                "failed new cold restore cannot be published"
            );
            assert_eq!(
                fs::read_dir(&parent).unwrap().count(),
                1,
                "partial cleanup must not affect preexisting private backup"
            );
            assert_eq!(
                fs::read(&prior).unwrap(),
                b"PRESERVE_COLD_RESTORE_OWNER_OLD_BYTES"
            );
            assert_eq!(
                verify_complete_directory(&published).unwrap().active_items,
                1
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_race_after_target_absence_check_does_not_replace_other_owner_state() {
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("destination-parent");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-race-source", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let final_target = parent.join("aw-restored-racing-owner");
        let mut raced = false;
        let result = restore_complete_cold_quarantined(&published, &final_target, |step, _| {
            if step == ColdRestoreStep::BeforeRename {
                raced = true;
                fs::create_dir(&final_target).unwrap();
                fs::set_permissions(&final_target, fs::Permissions::from_mode(0o700)).unwrap();
                fs::write(
                    final_target.join("original-owner-data"),
                    b"MUST_NOT_BE_REPLACED",
                )
                .unwrap();
            }
            Ok(())
        });
        assert!(raced && result.is_err());
        assert_eq!(
            fs::read(final_target.join("original-owner-data")).unwrap(),
            b"MUST_NOT_BE_REPLACED"
        );
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        assert_eq!(
            verify_complete_directory(&published).unwrap().active_items,
            1
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_post_rename_failure_preserves_quarantined_state_for_owner_inspection() {
        let root = TempRoot::new();
        let state = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("destination-parent");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let original_generation = owner.local_process_info().unwrap().store_generation;
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-post-rename", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let final_target = parent.join("aw-restored-uncertain-durability");
        let mut reached = false;
        let result = restore_complete_cold_quarantined(&published, &final_target, |step, _| {
            if step == ColdRestoreStep::AfterRenameBeforeParentSync {
                reached = true;
                return Err(Error::Denied("injected parent directory sync uncertainty"));
            }
            Ok(())
        });
        assert!(reached && result.is_err());
        assert!(
            final_target.join(COLD_QUARANTINE_MARKER).is_file(),
            "the newly published quarantined State must survive post-rename failure"
        );
        assert_eq!(
            inspect_restore_generation(&final_target.join("sessions.sqlite3")).unwrap(),
            original_generation + 1
        );
        assert!(
            SessionStore::open(&final_target).is_err(),
            "durability uncertainty does not imply permission to activate an Agent"
        );
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10s_existing_empty_directory_file_and_symlink_never_replaced() {
        use std::os::unix::fs::symlink;
        let root = TempRoot::new();
        let source = root.child("source-original");
        let bundles = root.child("bundles");
        let parent = root.child("target-parent");
        let owner = SessionStore::open(&source).unwrap();
        create_encrypted_queue(&owner);
        let published = capture_partial(&owner, &bundles, |_, _| Ok(()))
            .unwrap()
            .publish_complete(&owner, "aw-composite-existing-targets", |_, _| Ok(()))
            .unwrap();
        drop(owner);
        let dest = parent.join("aw-restored-must-not-overwrite");
        fs::create_dir(&dest).unwrap();
        assert!(restore_complete_cold_quarantined(&published, &dest, |_, _| Ok(())).is_err());
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
        fs::remove_dir(&dest).unwrap();
        fs::write(&dest, b"KEEP_EXISTING_PRIVATE_FILE").unwrap();
        assert!(restore_complete_cold_quarantined(&published, &dest, |_, _| Ok(())).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"KEEP_EXISTING_PRIVATE_FILE");
        fs::remove_file(&dest).unwrap();
        symlink(&published, &dest).unwrap();
        assert!(restore_complete_cold_quarantined(&published, &dest, |_, _| Ok(())).is_err());
        assert_eq!(fs::read_link(&dest).unwrap(), published);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_publish_rejects_existing_private_directory_file_and_symlink() {
        use std::os::unix::fs::symlink;
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let store = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&store);
        let name = "aw-composite-existing-target";
        let path = destination.join(name);
        let previous_dir = root.child("previous-real-dir");
        fs::rename(&previous_dir, &path).unwrap();
        fs::write(
            path.join("private-original"),
            b"DO_NOT_OVERWRITE_PREVIOUS_DIRECTORY",
        )
        .unwrap();
        let stage = capture_partial(&store, &destination, |_, _| Ok(())).unwrap();
        assert!(stage.publish_complete(&store, name, |_, _| Ok(())).is_err());
        assert_eq!(
            fs::read(path.join("private-original")).unwrap(),
            b"DO_NOT_OVERWRITE_PREVIOUS_DIRECTORY"
        );
        fs::remove_dir_all(&path).unwrap();
        fs::write(&path, b"DO_NOT_OVERWRITE_EXISTING_FILE").unwrap();
        let stage = capture_partial(&store, &destination, |_, _| Ok(())).unwrap();
        assert!(stage.publish_complete(&store, name, |_, _| Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"DO_NOT_OVERWRITE_EXISTING_FILE");
        fs::remove_file(&path).unwrap();
        symlink(&state, &path).unwrap();
        let stage = capture_partial(&store, &destination, |_, _| Ok(())).unwrap();
        assert!(stage.publish_complete(&store, name, |_, _| Ok(())).is_err());
        assert_eq!(fs::read_link(&path).unwrap(), state);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_published_complete_stays_source_independently_authentic() {
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let store = SessionStore::open(&state).unwrap();
        let active = create_encrypted_queue(&store);
        let vault = store.local_secret_store().unwrap();
        let other = provider_secret_ref("snapshot", "retired", 1).unwrap();
        vault.put(&other, b"SYNTHETIC_RETIRED_CIPHERTEXT").unwrap();
        let stage = capture_partial(&store, &destination, |_, _| Ok(())).unwrap();
        let name = "aw-composite-valid-v1";
        let completed = stage.publish_complete(&store, name, |_, _| Ok(())).unwrap();
        assert_eq!(completed, destination.join(name));
        assert!(completed.is_dir());
        assert_eq!(
            fs::read_dir(&destination).unwrap().count(),
            1,
            "published composite must not leave a partial directory behind"
        );
        let manifest = verify_complete_directory(&completed).unwrap();
        assert!(manifest.complete);
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.active_items, 1);
        assert_eq!(manifest.unreferenced_items, 1);
        assert!(completed.join("manifest.json").is_file());
        assert!(!completed.join("manifest.partial.json").exists());
        drop(vault);
        drop(store);
        fs::remove_dir_all(&state).unwrap();
        assert_eq!(
            verify_complete_directory(&completed).unwrap().active_items,
            1,
            "source-independent complete verification needs no original process, key or Store"
        );
        let recovered_vault = crate::LocalSecretStore::open(&completed).unwrap();
        assert_eq!(
            recovered_vault.get(&active).unwrap().as_slice(),
            b"ORIGINAL_SYNTHETIC_ENCRYPTED_PENDING_INPUT"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_fault_before_atomic_publication_preserves_old_backup_and_cleans_stage() {
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let store = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&store);
        let previous = destination.join("old-operator-backup");
        fs::write(&previous, b"PRESERVE_PREVIOUS_OWNER_BACKUP").unwrap();
        fs::set_permissions(&previous, fs::Permissions::from_mode(0o600)).unwrap();
        let final_name = "aw-composite-not-published";
        let stage = capture_partial(&store, &destination, |_, _| Ok(())).unwrap();
        let result = stage.publish_complete(&store, final_name, |step, _| {
            if step == PublishStep::BeforeRename {
                return Err(Error::Denied(
                    "injected failure after complete manifest before atomic rename",
                ));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(!destination.join(final_name).exists());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
        assert_eq!(
            fs::read(&previous).unwrap(),
            b"PRESERVE_PREVIOUS_OWNER_BACKUP"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_after_rename_failure_preserves_complete_directory_for_operator_inspection() {
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let store = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&store);
        let name = "aw-composite-after-rename-uncertain";
        let stage = capture_partial(&store, &destination, |_, _| Ok(())).unwrap();
        let result = stage.publish_complete(&store, name, |step, _| {
            if step == PublishStep::AfterRenameBeforeParentSync {
                return Err(Error::Denied("injected post-rename durability uncertainty"));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(
            destination.join(name).exists(),
            "a renamed owner-private archive must never be deleted on later sync uncertainty"
        );
        assert!(
            verify_complete_directory(&destination.join(name))
                .unwrap()
                .complete
        );
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_racing_destination_created_after_precheck_is_never_replaced() {
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let name = "aw-composite-atomic-race";
        let collision = destination.join(name);
        let stage = capture_partial(&owner, &destination, |_, _| Ok(())).unwrap();
        let mut raced = false;
        let attempt = stage.publish_complete(&owner, name, |boundary, _| {
            if boundary == PublishStep::BeforeRename {
                raced = true;
                // A realistic check-then-rename race: another owner operation
                // creates the final basename AFTER our first existence check.
                fs::create_dir(&collision).unwrap();
                fs::set_permissions(&collision, fs::Permissions::from_mode(0o700)).unwrap();
                fs::write(collision.join("prior-operator-data"), b"NEVER_REPLACED").unwrap();
            }
            Ok(())
        });
        assert!(
            raced && attempt.is_err(),
            "renameat2 NOREPLACE must refuse the mid-flight collision"
        );
        assert_eq!(
            fs::read(collision.join("prior-operator-data")).unwrap(),
            b"NEVER_REPLACED"
        );
        assert_eq!(
            fs::read_dir(&destination).unwrap().count(),
            1,
            "the failed private stage is cleaned; the competing final directory survives"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_untrusted_output_name_cannot_escape_private_owner_parent() {
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        for name in [
            "../escape",
            "aw-composite-../../other",
            "aw-composite-slash/child",
            "archive",
            ".partial",
            "aw-composite-🚩",
        ] {
            let stage = capture_partial(&owner, &destination, |_, _| Ok(())).unwrap();
            assert!(
                stage.publish_complete(&owner, name, |_, _| Ok(())).is_err(),
                "invalid operator path/name must fail before renaming: {name}"
            );
        }
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert!(!root.0.join("escape").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_complete_manifest_version_generation_and_counts_are_authoritative() {
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let name = "aw-composite-manifest-audit";
        let stage = capture_partial(&owner, &destination, |_, _| Ok(())).unwrap();
        let completed = stage.publish_complete(&owner, name, |_, _| Ok(())).unwrap();
        let original = verify_complete_directory(&completed).unwrap();
        assert!(original.complete);
        assert_eq!(
            original.restore_generation,
            owner.local_process_info().unwrap().store_generation
        );
        let path = completed.join("manifest.json");
        let saved = fs::read(&path).unwrap();
        let mutate = |f: &dyn Fn(&mut CompleteManifest)| {
            let mut changed = original.clone();
            f(&mut changed);
            fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
            assert!(verify_complete_directory(&completed).is_err());
            fs::write(&path, &saved).unwrap();
            assert!(verify_complete_directory(&completed).is_ok());
        };
        mutate(&|value| value.version = 2);
        mutate(&|value| value.restore_generation += 1);
        mutate(&|value| value.active_items = 0);
        mutate(&|value| value.format = "aw-INTERNAL-PARTIAL-NOT-RECOVERABLE".into());
        mutate(&|value| value.complete = false);
        assert_eq!(fs::read(&path).unwrap(), saved);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_real_process_exit_before_no_replace_rename_cannot_publish() {
        const SOURCE: &str = "AW_P6_10R_PRE_RENAME_SOURCE";
        const TARGET: &str = "AW_P6_10R_PRE_RENAME_TARGET";
        if let Some(root) = std::env::var_os(SOURCE) {
            let owner = SessionStore::open(Path::new(&root)).unwrap();
            let destination = PathBuf::from(std::env::var_os(TARGET).unwrap());
            let partial = capture_partial(&owner, &destination, |_, _| Ok(())).unwrap();
            let _ = partial.publish_complete(&owner, "aw-composite-never-rename", |step, _| {
                if step == PublishStep::BeforeRename {
                    std::process::exit(71); // No Rust destructors or SQL release
                }
                Ok(())
            });
            panic!("child must reach before-rename test boundary");
        }
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        drop(owner);
        let previous = destination.join("earlier-owner-private-backup");
        fs::write(&previous, b"PRIOR_USER_BACKUP_MUST_REMAIN").unwrap();
        fs::set_permissions(&previous, fs::Permissions::from_mode(0o600)).unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("composite_staging::tests::aw_p6_10r_real_process_exit_before_no_replace_rename_cannot_publish")
            .arg("--nocapture")
            .env(SOURCE, &state).env(TARGET, &destination)
            .output().unwrap();
        assert_eq!(
            child.status.code(),
            Some(71),
            "child did not reach original pre-rename boundary: stderr={}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(!destination.join("aw-composite-never-rename").exists());
        let partials = fs::read_dir(&destination)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|path| path.to_string_lossy().ends_with(".partial"))
            .collect::<Vec<_>>();
        assert_eq!(partials.len(), 1);
        assert!(
            partials[0].join("manifest.json").is_file(),
            "child left final-format bytes but did NOT atomically publish"
        );
        assert!(
            verify_complete_directory(&partials[0]).is_err(),
            ".partial with complete manifest must never be a published backup"
        );
        assert_eq!(
            fs::read(&previous).unwrap(),
            b"PRIOR_USER_BACKUP_MUST_REMAIN"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_real_process_exit_after_atomic_rename_leaves_valid_owner_archive() {
        const SOURCE: &str = "AW_P6_10R_POST_RENAME_SOURCE";
        const TARGET: &str = "AW_P6_10R_POST_RENAME_TARGET";
        const FINAL: &str = "aw-composite-exit-post-rename";
        if let Some(root) = std::env::var_os(SOURCE) {
            let owner = SessionStore::open(Path::new(&root)).unwrap();
            let destination = PathBuf::from(std::env::var_os(TARGET).unwrap());
            let stage = capture_partial(&owner, &destination, |_, _| Ok(())).unwrap();
            let _ = stage.publish_complete(&owner, FINAL, |step, _| {
                if step == PublishStep::AfterRenameBeforeParentSync {
                    std::process::exit(72);
                }
                Ok(())
            });
            panic!("child must reach after-rename test boundary");
        }
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        drop(owner);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("composite_staging::tests::aw_p6_10r_real_process_exit_after_atomic_rename_leaves_valid_owner_archive")
            .arg("--nocapture")
            .env(SOURCE, &state).env(TARGET, &destination)
            .output().unwrap();
        assert_eq!(
            child.status.code(),
            Some(72),
            "child did not reach post-rename boundary: stderr={}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
        let final_dir = destination.join(FINAL);
        assert!(final_dir.is_dir());
        let actual = verify_complete_directory(&final_dir).unwrap();
        assert!(actual.complete);
        assert_eq!(actual.active_items, 1);
        // No parent-dir fsync was reached: this proves only local surviving
        // bytes after a process exit, NOT power-loss durable publication.
        assert!(!final_dir.join("manifest.partial.json").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_restored_owner_generation_cannot_publish_old_capture() {
        // Capture under the genuine prior original Writer/Process generation,
        // then commit a supported owner-only SQLite restore. The restore
        // fences old process generations: a stale internal capture is not an
        // independent grant to publish as this NEW incarnation's archive.
        let root = TempRoot::new();
        let state = root.child("state");
        let destination = root.child("destination");
        let owner = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&owner);
        let before = owner.local_process_info().unwrap().store_generation;
        let partial = capture_partial(&owner, &destination, |_, _| Ok(())).unwrap();
        let snapshot = root.0.join("prior-original-sqlite-only-backup.sqlite3");
        owner.backup(&snapshot).unwrap();
        owner.restore_backup(&snapshot).unwrap();
        assert!(owner.local_process_info().unwrap().store_generation > before);
        assert!(
            partial
                .publish_complete(&owner, "aw-composite-stale-owner", |_, _| Ok(()))
                .is_err(),
            "previous restored owner generation cannot publish a stale captured Store identity"
        );
        assert_eq!(
            fs::read_dir(&destination).unwrap().count(),
            0,
            "owner generation mismatch must clean only its old private partial"
        );
        assert!(snapshot.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn aw_p6_10r_wrong_store_owner_cannot_publish_another_staged_identity() {
        let root = TempRoot::new();
        let original = root.child("original");
        let other = root.child("other");
        let destination = root.child("destination");
        let source = SessionStore::open(&original).unwrap();
        let wrong = SessionStore::open(&other).unwrap();
        create_encrypted_queue(&source);
        let stage = capture_partial(&source, &destination, |_, _| Ok(())).unwrap();
        assert!(stage
            .publish_complete(&wrong, "aw-composite-other-owner", |_, _| Ok(()))
            .is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    }

    #[test]
    fn aw_p6_10q_rehashed_tampered_active_ciphertext_cannot_pass_standalone_verification() {
        // Negative first: the original 10P verifier checks ONLY manifest
        // per-file SHA-256. A trusted operator could rewrite both ciphertext
        // and its manifest hash; independent original v33 AEAD and immutable
        // Queue payload SHA must still deny this coherent-looking partial.
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        let active = create_encrypted_queue(&store);
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        stage.verify().unwrap();
        let relative = format!("secrets/items/{active}.bin");
        let file = stage.directory.join(&relative);
        let mut changed = fs::read(&file).unwrap();
        *changed.last_mut().unwrap() ^= 0x20;
        fs::write(&file, &changed).unwrap();
        let manifest_file = stage.directory.join("manifest.partial.json");
        let mut manifest: PartialManifest =
            serde_json::from_slice(&fs::read(&manifest_file).unwrap()).unwrap();
        manifest
            .files
            .iter_mut()
            .find(|entry| entry.path == relative)
            .unwrap()
            .sha256 = format!("{:x}", Sha256::digest(&changed));
        fs::write(&manifest_file, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            stage.verify().is_err(),
            "a matching rewritten SHA-256 must not substitute for original AEAD and Queue audit"
        );
    }

    #[test]
    fn aw_p6_10q_rehashed_mismatched_master_cannot_pass_standalone_verification() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        create_encrypted_queue(&store);
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        let file = stage.directory.join("secrets/master.key");
        let wrong = [0x5a_u8; 32];
        assert_ne!(fs::read(&file).unwrap().as_slice(), wrong);
        fs::write(&file, wrong).unwrap();
        let manifest_file = stage.directory.join("manifest.partial.json");
        let mut manifest: PartialManifest =
            serde_json::from_slice(&fs::read(&manifest_file).unwrap()).unwrap();
        manifest
            .files
            .iter_mut()
            .find(|entry| entry.path == "secrets/master.key")
            .unwrap()
            .sha256 = format!("{:x}", Sha256::digest(wrong));
        fs::write(&manifest_file, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            stage.verify().is_err(),
            "an internally SHA-consistent wrong key cannot authenticate original ciphertext"
        );
    }

    #[test]
    fn aw_p6_10p_never_used_vault_capture_does_not_create_master_key() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        assert!(!state.join("secrets/master.key").exists());
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        let manifest = stage.verify().unwrap();
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.active_items, 0);
        assert_eq!(manifest.unreferenced_items, 0);
        assert!(!state.join("secrets/master.key").exists());
        assert!(!stage.directory.join("secrets/master.key").exists());
    }

    #[test]
    fn aw_p6_10p_staged_copy_tampering_rejected_but_original_unchanged() {
        let root = TempRoot::new();
        let state = root.child("state");
        let target = root.child("target");
        let store = SessionStore::open(&state).unwrap();
        let active = create_encrypted_queue(&store);
        let stage = capture_partial(&store, &target, |_, _| Ok(())).unwrap();
        stage.verify().unwrap();
        let staged = stage.directory.join(format!("secrets/items/{active}.bin"));
        let original = fs::read(&staged).unwrap();
        let mut altered = original;
        *altered.last_mut().unwrap() ^= 0x10;
        fs::write(&staged, altered).unwrap();
        assert!(stage.verify().is_err());
        assert!(
            store.local_secret_store().unwrap().get(&active).is_ok(),
            "staging failure may never modify original ciphertext"
        );
        let path = stage.directory.clone();
        drop(stage);
        assert!(!path.exists());
    }
}
