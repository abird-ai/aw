//! AW host binding for canonical mutation-private artifact ownership.
//! Persistent sessions store exact records beneath the existing private AW state
//! root; memory-only/direct SDK construction uses a process-local registry.
use crate::{Error, Result};
use aw_tools::filesystem::private_artifact::Registry;
#[cfg(unix)]
use aw_tools::filesystem::private_artifact::{Error as ArtifactError, Record, Store};
#[cfg(unix)]
use sha2::{Digest as _, Sha256};
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

static REGISTRIES: OnceLock<Mutex<HashMap<NamespaceKey, Weak<Registry>>>> = OnceLock::new();

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
}
#[cfg(unix)]
impl FileStore {
    fn new(state_root: &Path, root: &Path) -> Result<Self> {
        let dir = state_root
            .join("tool-private-artifacts")
            .join(root_digest(root));
        aw_platform::ensure_private_dir(&dir).map_err(Error::Io)?;
        Ok(Self { dir })
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
        aw_platform::ensure_private_dir(&self.dir)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let mut records = Vec::new();
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
            if !name.starts_with("artifact-") || !name.ends_with(".json") {
                return Err(ArtifactError::Store(format!(
                    "unexpected entry in private artifact store: {name}"
                )));
            }
            records.push(Self::read_record(&item.path())?);
        }
        Ok(records)
    }

    fn reserve(&self, record: &Record) -> std::result::Result<(), ArtifactError> {
        use std::io::Write as _;
        aw_platform::ensure_private_dir(&self.dir)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let path = self
            .record_path(record)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        let bytes = serde_json::to_vec(record)
            .map_err(|_| ArtifactError::Store("record serialization failed".into()))?;
        let mut file = aw_platform::open_private_new(&path)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        file.sync_all()
            .map_err(|error| ArtifactError::Store(error.to_string()))?;
        aw_platform::sync_parent(&path).map_err(|error| ArtifactError::Store(error.to_string()))?;
        Ok(())
    }

    fn retire(&self, record: &Record) -> std::result::Result<(), ArtifactError> {
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
    let key = namespace_key(&canonical, durable_state.as_deref());
    let cache = REGISTRIES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|_| Error::Protocol("private artifact registry cache poisoned"))?;
    if let Some(registry) = cache.get(&key).and_then(Weak::upgrade) {
        return crate::platform_fs(aw_platform::scoped_fs::Root::open_with_private_artifacts(
            &canonical, registry,
        ));
    }

    #[cfg(unix)]
    let registry = if let Some(state_root) = durable_state.as_deref() {
        Registry::open(Arc::new(FileStore::new(state_root, &canonical)?)).map_err(artifact_error)?
    } else {
        Registry::memory()
    };
    #[cfg(not(unix))]
    let registry = Registry::memory();

    let root = crate::platform_fs(aw_platform::scoped_fs::Root::open_with_private_artifacts(
        &canonical,
        registry.clone(),
    ))?;
    crate::platform_fs(root.reconcile_private_artifacts())?;
    cache.insert(key, Arc::downgrade(&registry));
    Ok(root)
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
    fn namespace_and_digest_preserve_non_utf8_root_identity() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;
        let a = PathBuf::from(OsString::from_vec(b"/tmp/root-\xff".to_vec()));
        let b = PathBuf::from(OsString::from_vec(b"/tmp/root-\xfe".to_vec()));
        assert_ne!(namespace_key(&a, None), namespace_key(&b, None));
        assert_ne!(root_digest(&a), root_digest(&b));
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
