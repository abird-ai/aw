//! AW host binding for canonical mutation-private artifact ownership.
//! Persistent sessions store exact records beneath the existing private AW state
//! root; memory-only/direct SDK construction uses a process-local registry.
use crate::{Error, Result};
use aw_tools::filesystem::private_artifact::Registry;
#[cfg(unix)]
use aw_tools::filesystem::private_artifact::{Error as ArtifactError, Record, Store};
#[cfg(unix)]
use sha2::{Digest as _, Sha256};
#[cfg(unix)]
use std::collections::BTreeMap;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NamespaceKey {
    state: Option<PathBuf>,
    root: PathBuf,
}

struct CachedRegistry {
    registry: Weak<Registry>,
    source_owned: bool,
}
static REGISTRIES: OnceLock<Mutex<HashMap<NamespaceKey, CachedRegistry>>> = OnceLock::new();

fn with_registry<T>(
    key: NamespaceKey,
    source_owned: bool,
    create: impl FnOnce() -> Result<Arc<Registry>>,
    open: impl FnOnce(Arc<Registry>, bool) -> Result<T>,
) -> Result<T> {
    let cache = REGISTRIES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|_| Error::Protocol("private artifact registry cache poisoned"))?;
    if let Some(entry) = cache.get(&key) {
        if let Some(registry) = entry.registry.upgrade() {
            if entry.source_owned != source_owned {
                return Err(Error::Conflict(
                    "private artifact namespace belongs to another recovery owner",
                ));
            }
            return open(registry, false);
        }
    }
    let registry = create()?;
    let result = open(registry.clone(), true)?;
    cache.insert(
        key,
        CachedRegistry {
            registry: Arc::downgrade(&registry),
            source_owned,
        },
    );
    Ok(result)
}

#[cfg(unix)]
fn artifact_error(error: ArtifactError) -> Error {
    Error::Config(format!("private mutation artifact state: {error}"))
}

fn namespace_key(root: &Path, state: Option<&Path>) -> NamespaceKey {
    NamespaceKey {
        state: state.map(Path::to_path_buf),
        root: root.to_path_buf(),
    }
}

#[cfg(unix)]
fn root_digest(root: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    let digest = Sha256::digest(root.as_os_str().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(unix)]
struct FileStore {
    dir: PathBuf,
    source_owned: bool,
}
#[cfg(unix)]
impl FileStore {
    // This marker selects the existing registry's recovery owner, not
    // physical quiescence or an effect receipt. A legacy ambient opener
    // must never automatically clean source-owned records after restart.
    const SOURCE_OWNER: &'static str = ".source-owned-v1";
    const SOURCE_OWNER_BYTES: &'static [u8] = b"aw-source-artifacts-v1\n";

    fn directory(state_root: &Path, root: &Path) -> PathBuf {
        state_root
            .join("tool-private-artifacts")
            .join(root_digest(root))
    }

    fn new(state_root: &Path, root: &Path) -> Result<Self> {
        let dir = Self::directory(state_root, root);
        aw_platform::ensure_private_dir(&dir).map_err(Error::Io)?;
        let store = Self {
            dir,
            source_owned: false,
        };
        store.require_owner()?;
        Ok(store)
    }

    fn source(state_root: &Path, root: &Path, create: bool) -> Result<Self> {
        let dir = Self::directory(state_root, root);
        if create {
            aw_platform::ensure_private_dir(&dir)?;
        } else {
            aw_platform::validate_private_dir(&dir)?;
        }
        let store = Self {
            dir,
            source_owned: true,
        };
        let marker = store.dir.join(Self::SOURCE_OWNER);
        match std::fs::symlink_metadata(&marker) {
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::read_dir(&store.dir)?.next().transpose()?.is_some() {
                    return Err(Error::Conflict(
                        "cannot adopt ambient private artifact records",
                    ));
                }
                use std::io::Write;
                let mut file = aw_platform::open_private_new(&marker)?;
                file.write_all(Self::SOURCE_OWNER_BYTES)?;
                file.sync_all()?;
                aw_platform::sync_parent(&marker)?;
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        store.require_owner()?;
        Ok(store)
    }

    fn require_owner(&self) -> Result<()> {
        let marker = self.dir.join(Self::SOURCE_OWNER);
        if self.source_owned {
            use std::io::Read;
            let mut bytes = Vec::new();
            aw_platform::open_private_existing(&marker, false)?
                .take((Self::SOURCE_OWNER_BYTES.len() + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes != Self::SOURCE_OWNER_BYTES {
                return Err(Error::Conflict("invalid source artifact namespace owner"));
            }
        } else {
            match std::fs::symlink_metadata(&marker) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => {
                    return Err(Error::Conflict(
                        "source artifact namespace requires original source recovery owner",
                    ))
                }
            }
        }
        Ok(())
    }

    fn record_path(&self, record: &Record) -> Result<PathBuf> {
        let bytes = serde_json::to_vec(record)
            .map_err(|_| Error::Protocol("private artifact record serialization failed"))?;
        let digest = Sha256::digest(&bytes);
        let name = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(self.dir.join(format!("artifact-{name}.json")))
    }

    fn open_pending_record(&self) -> Result<(PathBuf, std::fs::File)> {
        for _ in 0..16 {
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|_| {
                Error::Config("OS randomness unavailable for private artifact state".into())
            })?;
            let suffix = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let path = self.dir.join(format!(".artifact-pending-{suffix}.tmp"));
            match aw_platform::open_private_new(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(Error::Io(error)),
            }
        }
        Err(Error::Conflict(
            "cannot allocate private artifact pending record name",
        ))
    }

    fn is_pending_name(name: &str) -> bool {
        const PREFIX: &str = ".artifact-pending-";
        const SUFFIX: &str = ".tmp";
        let Some(body) = name
            .strip_prefix(PREFIX)
            .and_then(|value| value.strip_suffix(SUFFIX))
        else {
            return false;
        };
        body.len() == 32 && body.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn reclaim_pending_record(&self, path: &Path) -> std::result::Result<(), ArtifactError> {
        let file = aw_platform::open_private_existing(path, false)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        drop(file);
        std::fs::remove_file(path).map_err(|error| ArtifactError::Store(error.to_string()))?;
        aw_platform::sync_parent(path).map_err(|error| ArtifactError::Store(error.to_string()))?;
        Ok(())
    }

    fn read_record(path: &Path) -> std::result::Result<Record, ArtifactError> {
        use std::io::Read as _;
        let mut file = aw_platform::open_private_existing(path, false)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let metadata = file
            .metadata()
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        if metadata.len() > 16 * 1024 {
            return Err(ArtifactError::Store(
                "private artifact record exceeds 16 KiB".into(),
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        serde_json::from_slice(&bytes)
            .map_err(|_| ArtifactError::Store("malformed private artifact record".into()))
    }
}

#[cfg(unix)]
impl Store for FileStore {
    fn load(&self) -> std::result::Result<Vec<Record>, ArtifactError> {
        if self.source_owned {
            aw_platform::validate_private_dir(&self.dir)
        } else {
            aw_platform::ensure_private_dir(&self.dir)
        }
        .map_err(|error| ArtifactError::Store(error.to_string()))?;
        self.require_owner()
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let mut records = BTreeMap::<
            (
                aw_tools::filesystem::private_artifact::ObjectIdentity,
                String,
            ),
            (PathBuf, Record),
        >::new();
        for item in
            std::fs::read_dir(&self.dir).map_err(|error| ArtifactError::Store(error.to_string()))?
        {
            let item = item.map_err(|error| ArtifactError::Store(error.to_string()))?;
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                return Err(ArtifactError::Store(
                    "non-UTF8 private artifact record filename".into(),
                ));
            };
            if self.source_owned && name == Self::SOURCE_OWNER {
                continue;
            }
            if Self::is_pending_name(name) {
                self.reclaim_pending_record(&item.path())?;
                continue;
            }
            if !name.starts_with("artifact-") || !name.ends_with(".json") {
                return Err(ArtifactError::Store(format!(
                    "unexpected entry in private artifact store: {name}"
                )));
            }
            let path = item.path();
            let record = Self::read_record(&path)?;
            let key = (record.parent, record.name.clone());
            if let Some((existing_path, existing)) = records.get(&key).cloned() {
                let compatible = existing.kind == record.kind
                    && existing.parent_path == record.parent_path
                    && existing.object.is_none() != record.object.is_none();
                if !compatible {
                    return Err(ArtifactError::Conflict);
                }
                let (provisional_path, finalized_path, finalized) = if existing.object.is_none() {
                    (existing_path, path, record)
                } else {
                    (path, existing_path, existing)
                };
                // Both record files were individually validated above. Once the
                // finalized record is present, the provisional one is obsolete.
                std::fs::remove_file(&provisional_path)
                    .map_err(|error| ArtifactError::Store(error.to_string()))?;
                aw_platform::sync_parent(&provisional_path)
                    .map_err(|error| ArtifactError::Store(error.to_string()))?;
                records.insert(key, (finalized_path, finalized));
            } else {
                records.insert(key, (path, record));
            }
        }
        Ok(records.into_values().map(|(_, record)| record).collect())
    }

    fn reserve(&self, record: &Record) -> std::result::Result<(), ArtifactError> {
        use std::io::Write as _;
        // A source-owned namespace cannot be silently recreated after
        // loss of its durable state or recovery-owner marker.
        if self.source_owned {
            aw_platform::validate_private_dir(&self.dir)
        } else {
            aw_platform::ensure_private_dir(&self.dir)
        }
        .map_err(|error| ArtifactError::Store(error.to_string()))?;
        self.require_owner()
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let path = self
            .record_path(record)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let (pending, mut file) = self
            .open_pending_record()
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let bytes = serde_json::to_vec(record)
            .map_err(|_| ArtifactError::Store("record serialization failed".into()))?;
        let mut published = false;
        let result = (|| -> std::result::Result<(), ArtifactError> {
            file.write_all(&bytes)
                .map_err(|error| ArtifactError::Store(error.to_string()))?;
            file.sync_all()
                .map_err(|error| ArtifactError::Store(error.to_string()))?;
            drop(file);
            match aw_platform::publish_private_new(&pending, &path) {
                Ok(()) => published = true,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(ArtifactError::Conflict)
                }
                Err(error) => return Err(ArtifactError::Store(error.to_string())),
            }
            aw_platform::sync_parent(&path)
                .map_err(|error| ArtifactError::Store(error.to_string()))?;
            Ok(())
        })();
        if !published {
            let _ = std::fs::remove_file(&pending);
        }
        result
    }

    fn finalize(&self, before: &Record, after: &Record) -> std::result::Result<(), ArtifactError> {
        if before.kind != after.kind
            || before.parent_path != after.parent_path
            || before.parent != after.parent
            || before.name != after.name
            || before.object.is_some()
            || after.object.is_none()
        {
            return Err(ArtifactError::Conflict);
        }
        let final_path = self
            .record_path(after)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        match self.reserve(after) {
            Ok(()) => {}
            Err(ArtifactError::Conflict) => {
                if Self::read_record(&final_path)? != *after {
                    return Err(ArtifactError::Conflict);
                }
            }
            Err(error) => return Err(error),
        }
        // Finalized ownership is already durable. Failure to remove the old
        // provisional record is recoverable: load() coalesces the exact pair.
        let _ = self.retire(before);
        Ok(())
    }

    fn retire(&self, record: &Record) -> std::result::Result<(), ArtifactError> {
        self.require_owner()
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let path = self
            .record_path(record)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let current = Self::read_record(&path)?;
        if current != *record {
            return Err(ArtifactError::Conflict);
        }
        std::fs::remove_file(&path).map_err(|error| ArtifactError::Store(error.to_string()))?;
        aw_platform::sync_parent(&path).map_err(|error| ArtifactError::Store(error.to_string()))?;
        Ok(())
    }
}

pub(crate) fn open_root(
    path: &Path,
    state_root: Option<&Path>,
) -> Result<aw_platform::scoped_fs::Root> {
    let canonical = path.canonicalize()?;
    #[cfg(unix)]
    let durable_state = match state_root {
        Some(path) if aw_platform::secure_private_files_supported() => {
            aw_platform::ensure_private_dir(path).map_err(Error::Io)?;
            Some(path.canonicalize()?)
        }
        Some(_) | None => None,
    };
    #[cfg(not(unix))]
    let durable_state: Option<PathBuf> = {
        // Persistent transcript/state support does not imply SecretStore-grade
        // custody for mutation recovery records. Until the platform supplies
        // verified private files, retain volatile stage ownership and simply do
        // not expose operations that require durable tombstones.
        let _ = state_root;
        None
    };
    with_registry(
        namespace_key(&canonical, durable_state.as_deref()),
        false,
        || {
            #[cfg(unix)]
            if let Some(state_root) = durable_state.as_deref() {
                return Registry::open(Arc::new(FileStore::new(state_root, &canonical)?))
                    .map_err(artifact_error);
            }
            Ok(Registry::memory())
        },
        |registry, fresh| {
            let root = crate::platform_fs(
                aw_platform::scoped_fs::Root::open_with_private_artifacts(&canonical, registry),
            )?;
            if fresh {
                crate::platform_fs(root.reconcile_private_artifacts())?;
            }
            Ok(root)
        },
    )
}

/// Provision the existing durable artifact namespace for an independently
/// approved source directory. This never reconciles source contents. The
/// host owns exclusive protection of the private state and source roots.
pub fn open_source_artifacts(root: &Path, state_root: &Path) -> Result<Arc<Registry>> {
    source_artifacts(root, state_root, true)
}

/// Recovery may reopen only an already-provisioned namespace. Missing or
/// unsupported durable state is not an empty registry or quiescence proof.
pub fn reopen_source_artifacts(root: &Path, state_root: &Path) -> Result<Arc<Registry>> {
    source_artifacts(root, state_root, false)
}

#[cfg(unix)]
fn source_artifacts(root: &Path, state_root: &Path, create: bool) -> Result<Arc<Registry>> {
    if !aw_platform::secure_private_files_supported()
        || root.canonicalize()? != root
        || !root.is_dir()
        || state_root.canonicalize()? != state_root
        || root.starts_with(state_root)
        || state_root.starts_with(root)
    {
        return Err(Error::Conflict(
            "source artifacts require exact disjoint durable private state",
        ));
    }
    aw_platform::validate_private_dir(state_root)?;
    with_registry(
        namespace_key(root, Some(state_root)),
        true,
        || {
            Registry::open(Arc::new(FileStore::source(state_root, root, create)?))
                .map_err(artifact_error)
        },
        |registry, _| {
            // Recovery cannot silently adopt missing state even if a live
            // registry remains cached. Opening does not touch source files.
            FileStore::source(state_root, root, false)?;
            Ok(registry)
        },
    )
}

#[cfg(not(unix))]
fn source_artifacts(_root: &Path, _state_root: &Path, _create: bool) -> Result<Arc<Registry>> {
    Err(Error::Conflict(
        "durable source artifact custody unavailable on this platform",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    struct Temp {
        root: PathBuf,
        workspace: PathBuf,
        state: PathBuf,
    }
    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "aw-private-artifact-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let workspace = root.join("workspace");
            let state = root.join("state");
            fs::create_dir_all(&workspace).unwrap();
            aw_platform::ensure_private_dir(&state).unwrap();
            Self {
                root,
                workspace,
                state,
            }
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fresh_registry(temp: &Temp) -> Arc<Registry> {
        let workspace = temp.workspace.canonicalize().unwrap();
        Registry::open(Arc::new(FileStore::new(&temp.state, &workspace).unwrap())).unwrap()
    }

    #[test]
    fn source_namespace_reopens_without_ambient_cleanup() {
        use aw_tools::filesystem::private_artifact::{Kind, ObjectIdentity};
        use std::os::unix::fs::MetadataExt;
        let temp = Temp::new("source-owner");
        let root = temp.workspace.canonicalize().unwrap();
        let state = temp.state.canonicalize().unwrap();
        assert!(reopen_source_artifacts(&root, &state).is_err());
        let registry = open_source_artifacts(&root, &state).unwrap();
        assert!(registry.is_durable());
        assert!(Arc::ptr_eq(
            &registry,
            &reopen_source_artifacts(&root, &state).unwrap()
        ));
        assert!(open_root(&root, Some(&state)).is_err());

        let identity = |path: &Path| {
            let meta = fs::symlink_metadata(path).unwrap();
            ObjectIdentity {
                volume: meta.dev(),
                object: meta.ino(),
            }
        };
        let name = ".aw-stage-source-fixture.tmp";
        let mut reservation = registry
            .reserve_name(Kind::Stage, String::new(), identity(&root), name.into())
            .unwrap();
        let private = root.join(name);
        let file = aw_platform::open_private_new(&private).unwrap();
        let lease = reservation.finalize(identity(&private)).unwrap();
        drop(file);
        drop(lease);
        drop(reservation);
        drop(registry);
        // No live cache owner: the persisted namespace discriminator
        // still prevents legacy startup from cleaning the source.
        assert!(open_root(&root, Some(&state)).is_err());
        let registry = reopen_source_artifacts(&root, &state).unwrap();
        assert_eq!(registry.records().unwrap().len(), 1);
        assert!(private.is_file());
        let capability =
            aw_platform::scoped_fs::Root::open_with_private_artifacts(&root, registry.clone())
                .unwrap();
        capability.reconcile_private_artifacts().unwrap();
        assert!(!private.exists());
        assert!(registry.records().unwrap().is_empty());
    }

    #[test]
    fn namespace_and_digest_preserve_non_utf8_root_identity() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;
        let a = PathBuf::from(OsString::from_vec(b"/tmp/root-\xff".to_vec()));
        let b = PathBuf::from(OsString::from_vec(b"/tmp/root-\xfe".to_vec()));
        assert_ne!(namespace_key(&a, None), namespace_key(&b, None));
        assert_ne!(root_digest(&a), root_digest(&b));
    }

    #[test]
    fn pending_record_from_interrupted_publication_is_reclaimed_before_load() {
        use std::io::Write as _;
        let temp = Temp::new("pending-record");
        let workspace = temp.workspace.canonicalize().unwrap();
        let store = FileStore::new(&temp.state, &workspace).unwrap();
        let pending = store
            .dir
            .join(".artifact-pending-0123456789abcdef0123456789abcdef.tmp");
        let mut file = aw_platform::open_private_new(&pending).unwrap();
        file.write_all(b"{partial").unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(store.load().unwrap().is_empty());
        assert!(!pending.exists(), "stale pending record must be reclaimed");
    }

    #[test]
    fn malformed_pending_lookalike_fails_closed_instead_of_being_deleted() {
        let temp = Temp::new("pending-lookalike");
        let workspace = temp.workspace.canonicalize().unwrap();
        let store = FileStore::new(&temp.state, &workspace).unwrap();
        let lookalike = store.dir.join(".artifact-pending-not-ours.tmp");
        aw_platform::open_private_new(&lookalike).unwrap();
        assert!(store.load().is_err());
        assert!(lookalike.exists());
    }

    #[test]
    fn durable_record_publication_is_complete_and_no_replace() {
        let temp = Temp::new("record-publish");
        let workspace = temp.workspace.canonicalize().unwrap();
        let store = FileStore::new(&temp.state, &workspace).unwrap();
        let record = Record {
            kind: aw_tools::filesystem::private_artifact::Kind::Stage,
            parent_path: ".".into(),
            parent: aw_tools::filesystem::private_artifact::ObjectIdentity {
                volume: 1,
                object: 2,
            },
            name: ".aw-stage-fixture.tmp".into(),
            object: Some(aw_tools::filesystem::private_artifact::ObjectIdentity {
                volume: 1,
                object: 3,
            }),
        };
        store.reserve(&record).unwrap();
        assert_eq!(store.load().unwrap(), vec![record.clone()]);
        assert_eq!(store.reserve(&record), Err(ArtifactError::Conflict));
        assert_eq!(store.load().unwrap(), vec![record]);
        assert!(fs::read_dir(&store.dir).unwrap().flatten().all(|entry| {
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".artifact-pending-")
        }));
    }

    #[test]
    fn durable_provisional_record_finalizes_and_load_coalesces_crash_pair() {
        let temp = Temp::new("record-finalize");
        let workspace = temp.workspace.canonicalize().unwrap();
        let store = FileStore::new(&temp.state, &workspace).unwrap();
        let parent = aw_tools::filesystem::private_artifact::ObjectIdentity {
            volume: 1,
            object: 2,
        };
        let before = Record {
            kind: aw_tools::filesystem::private_artifact::Kind::Stage,
            parent_path: ".".into(),
            parent,
            name: ".aw-stage-fixture.tmp".into(),
            object: None,
        };
        let mut after = before.clone();
        after.object = Some(aw_tools::filesystem::private_artifact::ObjectIdentity {
            volume: 1,
            object: 3,
        });
        store.reserve(&before).unwrap();
        // Simulate the durable half of finalize followed by a crash before the
        // provisional record is retired.
        store.reserve(&after).unwrap();
        assert_eq!(store.load().unwrap(), vec![after.clone()]);
        assert_eq!(store.load().unwrap(), vec![after]);
    }

    #[test]
    fn restart_reconciles_exact_orphan_without_publishing_staged_bytes() {
        let temp = Temp::new("cleanup");
        fs::write(temp.workspace.join("target.txt"), b"old").unwrap();
        let registry = fresh_registry(&temp);
        let root = aw_platform::scoped_fs::Root::open_with_private_artifacts(
            &temp.workspace,
            registry.clone(),
        )
        .unwrap();
        let staged = root.stage_replace("target.txt", b"new", &|| false).unwrap();
        let record = registry.records().unwrap().pop().unwrap();
        let private = temp.workspace.join(&record.name);
        assert!(private.exists());
        assert_eq!(fs::read(temp.workspace.join("target.txt")).unwrap(), b"old");

        // Abrupt process death: no StagedWrite/Lease Drop cleanup runs.
        std::mem::forget(staged);
        drop(root);
        drop(registry);

        let recovered = fresh_registry(&temp);
        let root = aw_platform::scoped_fs::Root::open_with_private_artifacts(
            &temp.workspace,
            recovered.clone(),
        )
        .unwrap();
        root.reconcile_private_artifacts().unwrap();
        assert!(!private.exists());
        assert!(recovered.records().unwrap().is_empty());
        assert_eq!(
            fs::read(temp.workspace.join("target.txt")).unwrap(),
            b"old",
            "unpublished staged bytes must never become the target during recovery"
        );
    }

    #[test]
    fn restart_never_deletes_same_name_user_replacement_with_different_identity() {
        let temp = Temp::new("replacement");
        fs::write(temp.workspace.join("target.txt"), b"old").unwrap();
        let registry = fresh_registry(&temp);
        let root = aw_platform::scoped_fs::Root::open_with_private_artifacts(
            &temp.workspace,
            registry.clone(),
        )
        .unwrap();
        let staged = root.stage_replace("target.txt", b"new", &|| false).unwrap();
        let record = registry.records().unwrap().pop().unwrap();
        let private = temp.workspace.join(&record.name);
        std::mem::forget(staged);
        drop(root);
        drop(registry);

        fs::remove_file(&private).unwrap();
        fs::write(&private, b"user replacement").unwrap();

        let recovered = fresh_registry(&temp);
        let root = aw_platform::scoped_fs::Root::open_with_private_artifacts(
            &temp.workspace,
            recovered.clone(),
        )
        .unwrap();
        root.reconcile_private_artifacts().unwrap();
        assert_eq!(fs::read(&private).unwrap(), b"user replacement");
        assert!(recovered.records().unwrap().is_empty());
        assert_eq!(fs::read(temp.workspace.join("target.txt")).unwrap(), b"old");
    }
}
