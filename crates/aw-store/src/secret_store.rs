use super::*;
use fs2::FileExt;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use zeroize::Zeroizing;

const SECRET_MAGIC: &[u8; 6] = b"AWSEC1";
const NONCE_LEN: usize = 12;
const MASTER_KEY_LEN: usize = 32;
pub const MAX_SECRET_BYTES: usize = 64 * 1024;

/// AW-P6-10N: stable owner-only advisory gate at the root of the existing
/// SecretStore, NOT under its items/ directory. One inode is preserved across
/// all direct vault handles, original owner processes and owner backup/restore.
/// Never unlink/recreate this file during runtime; the OS releases its lock
/// automatically if the owning process exits. Other same-UID code can bypass
/// advisory locks, outside the original trusted-host security boundary.
const MUTATION_FENCE_FILE: &str = ".mutation.lock";
pub(crate) fn open_vault_mutation_file(state_dir: &Path) -> Result<File> {
    let secret_root = state_dir.join("secrets");
    aw_platform::ensure_private_dir(&secret_root).map_err(Error::Io)?;
    aw_platform::open_private_rw_create(&secret_root.join(MUTATION_FENCE_FILE)).map_err(Error::Io)
}

fn try_vault_writer(state_dir: &Path) -> Result<File> {
    let file = open_vault_mutation_file(state_dir)?;
    FileExt::try_lock_shared(&file).map_err(|_| {
        Error::Conflict("SecretStore mutation fenced by original owner maintenance")
    })?;
    Ok(file)
}

pub trait SecretStore: Send + Sync {
    fn put(&self, secret_ref: &str, secret: &[u8]) -> Result<()>;
    fn get(&self, secret_ref: &str) -> Result<Zeroizing<Vec<u8>>>;
    fn delete(&self, secret_ref: &str) -> Result<()>;
    fn exists(&self, secret_ref: &str) -> Result<bool>;
}

fn valid_secret_ref(value: &str) -> Result<()> {
    if value.len() != 68
        || !value.starts_with("sec_")
        || !value[4..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::Config("invalid opaque SecretStore reference".into()));
    }
    Ok(())
}

fn same_secret(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (&left, &right) in a.iter().zip(b) {
        difference |= left ^ right;
    }
    difference == 0
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| Error::Storage("OS randomness unavailable"))?;
    Ok(out)
}

fn read_private_file(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut file = aw_platform::open_private_existing(path, false).map_err(Error::Io)?;
    let mut bytes = Vec::new();
    (&mut file).take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(Error::Limit("SecretStore blob exceeds allowed size"));
    }
    Ok(bytes)
}

fn create_master_key(path: &Path) -> Result<[u8; MASTER_KEY_LEN]> {
    let key = random_bytes::<MASTER_KEY_LEN>()?;
    let mut file = aw_platform::open_private_new(path).map_err(Error::Io)?;
    file.write_all(&key)?;
    file.sync_all()?;
    aw_platform::sync_parent(path)?;
    Ok(key)
}

#[derive(Clone)]
pub struct LocalSecretStore {
    root: PathBuf,
    state_dir: PathBuf,
    key: Arc<LessSafeKey>,
}

impl LocalSecretStore {
    pub fn open(state_dir: &Path) -> Result<Self> {
        if !aw_platform::secure_private_files_supported() {
            return Err(Error::Denied(
                "local encrypted SecretStore requires platform-verified owner-only private files",
            ));
        }
        let root = state_dir.join("secrets");
        let items = root.join("items");
        aw_platform::ensure_private_dir(&root).map_err(Error::Io)?;
        aw_platform::ensure_private_dir(&items).map_err(Error::Io)?;
        let master_path = root.join("master.key");
        let master = Zeroizing::new(match read_private_file(&master_path, MASTER_KEY_LEN) {
            Ok(bytes) => bytes
                .try_into()
                .map_err(|_| Error::Storage("SecretStore master key has invalid length"))?,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                // Generating a master key is a persistent vault mutation as
                // well. Acquire the original stable fence FIRST so no fresh
                // unrelated master can appear inside a backup/restore cut.
                // Exclusive rather than shared also serializes simultaneous
                // empty-vault initializations from direct handles.
                let file = open_vault_mutation_file(state_dir)?;
                FileExt::try_lock_exclusive(&file).map_err(|_| {
                    Error::Conflict("SecretStore key initialization fenced by owner maintenance")
                })?;
                match read_private_file(&master_path, MASTER_KEY_LEN) {
                    Ok(bytes) => bytes
                        .try_into()
                        .map_err(|_| Error::Storage("SecretStore master key has invalid length"))?,
                    Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                        if std::fs::read_dir(&items)?.next().is_some() {
                            return Err(Error::Denied(
                                "SecretStore master key is missing while encrypted items exist; restore the key or explicitly revoke/recreate those credentials",
                            ));
                        }
                        create_master_key(&master_path)?
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        });
        let key = LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, master.as_ref())
                .map_err(|_| Error::Storage("cannot initialize SecretStore cipher"))?,
        );
        Ok(Self {
            root: items,
            state_dir: state_dir.to_owned(),
            key: Arc::new(key),
        })
    }

    fn path(&self, secret_ref: &str) -> Result<PathBuf> {
        valid_secret_ref(secret_ref)?;
        Ok(self.root.join(format!("{secret_ref}.bin")))
    }

    fn write_atomic(&self, target: &Path, bytes: &[u8]) -> Result<()> {
        let temp = self.root.join(format!(
            ".secret-{}.tmp",
            random_bytes::<8>()?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ));
        let mut file = aw_platform::open_private_new(&temp).map_err(Error::Io)?;
        if let Err(error) = (|| -> Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            // Link-then-unlink gives create-new semantics for the final ref:
            // an existing credential version can never be silently overwritten.
            std::fs::hard_link(&temp, target)?;
            std::fs::remove_file(&temp)?;
            aw_platform::sync_parent(target)?;
            Ok(())
        })() {
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
        Ok(())
    }
}

impl SecretStore for LocalSecretStore {
    fn put(&self, secret_ref: &str, secret: &[u8]) -> Result<()> {
        // Keep a shared original vault fence THROUGH the atomic encrypted
        // write and directory fsync; an owner maintenance cut takes exclusive.
        // Cached vault handles and other cooperating owner processes must
        // reacquire on EVERY mutation, never rely on their open-time grant.
        let _mutation = try_vault_writer(&self.state_dir)?;
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(Error::Limit("secret must contain 1..65536 bytes"));
        }
        let target = self.path(secret_ref)?;
        if self.exists(secret_ref)? {
            let existing = self.get(secret_ref)?;
            if same_secret(existing.as_slice(), secret) {
                return Ok(());
            }
            return Err(Error::Conflict(
                "SecretStore reference already exists with different secret material",
            ));
        }
        let nonce_bytes = random_bytes::<NONCE_LEN>()?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut encrypted = secret.to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::from(secret_ref.as_bytes()), &mut encrypted)
            .map_err(|_| Error::Storage("SecretStore encryption failed"))?;
        let mut blob = Vec::with_capacity(SECRET_MAGIC.len() + NONCE_LEN + encrypted.len());
        blob.extend_from_slice(SECRET_MAGIC);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&encrypted);
        self.write_atomic(&target, &blob)?;
        Ok(())
    }

    fn get(&self, secret_ref: &str) -> Result<Zeroizing<Vec<u8>>> {
        let path = self.path(secret_ref)?;
        let blob = read_private_file(
            &path,
            SECRET_MAGIC.len() + NONCE_LEN + MAX_SECRET_BYTES + CHACHA20_POLY1305.tag_len(),
        )?;
        if blob.len() < SECRET_MAGIC.len() + NONCE_LEN + CHACHA20_POLY1305.tag_len()
            || &blob[..SECRET_MAGIC.len()] != SECRET_MAGIC
        {
            return Err(Error::Storage("invalid SecretStore blob"));
        }
        let nonce_offset = SECRET_MAGIC.len();
        let nonce_bytes: [u8; NONCE_LEN] = blob[nonce_offset..nonce_offset + NONCE_LEN]
            .try_into()
            .map_err(|_| Error::Storage("invalid SecretStore nonce"))?;
        let mut encrypted = blob[nonce_offset + NONCE_LEN..].to_vec();
        let plaintext = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(secret_ref.as_bytes()),
                &mut encrypted,
            )
            .map_err(|_| Error::Denied("SecretStore authentication failed"))?;
        Ok(Zeroizing::new(plaintext.to_vec()))
    }

    fn delete(&self, secret_ref: &str) -> Result<()> {
        let _mutation = try_vault_writer(&self.state_dir)?;
        let path = self.path(secret_ref)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                aw_platform::sync_parent(&path)?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn exists(&self, secret_ref: &str) -> Result<bool> {
        let path = self.path(secret_ref)?;
        match read_private_file(
            &path,
            SECRET_MAGIC.len() + NONCE_LEN + MAX_SECRET_BYTES + CHACHA20_POLY1305.tag_len(),
        ) {
            Ok(_) => Ok(true),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl SecretStore for MemorySecretStore {
    fn put(&self, secret_ref: &str, secret: &[u8]) -> Result<()> {
        valid_secret_ref(secret_ref)?;
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(Error::Limit("secret must contain 1..65536 bytes"));
        }
        let mut values = self
            .values
            .lock()
            .map_err(|_| Error::Storage("memory SecretStore lock poisoned"))?;
        if values.contains_key(secret_ref) {
            return Err(Error::Conflict(
                "SecretStore reference already exists; credential versions are write-once",
            ));
        }
        values.insert(secret_ref.to_owned(), secret.to_vec());
        Ok(())
    }

    fn get(&self, secret_ref: &str) -> Result<Zeroizing<Vec<u8>>> {
        valid_secret_ref(secret_ref)?;
        self.values
            .lock()
            .map_err(|_| Error::Storage("memory SecretStore lock poisoned"))?
            .get(secret_ref)
            .cloned()
            .map(Zeroizing::new)
            .ok_or(Error::Denied("SecretStore entry unavailable"))
    }

    fn delete(&self, secret_ref: &str) -> Result<()> {
        valid_secret_ref(secret_ref)?;
        self.values
            .lock()
            .map_err(|_| Error::Storage("memory SecretStore lock poisoned"))?
            .remove(secret_ref);
        Ok(())
    }

    fn exists(&self, secret_ref: &str) -> Result<bool> {
        valid_secret_ref(secret_ref)?;
        Ok(self
            .values
            .lock()
            .map_err(|_| Error::Storage("memory SecretStore lock poisoned"))?
            .contains_key(secret_ref))
    }
}

pub fn provider_secret_ref(store_id: &str, account_id: &str, version: u64) -> Result<String> {
    if version == 0 {
        return Err(Error::Config("secret version must be positive".into()));
    }
    label(account_id, 128)?;
    let mut digest = Sha256::new();
    digest.update(b"aw-provider-secret-v1\0");
    digest.update(store_id.as_bytes());
    digest.update([0]);
    digest.update(account_id.as_bytes());
    digest.update([0]);
    digest.update(version.to_be_bytes());
    Ok(format!("sec_{:x}", digest.finalize()))
}

impl SessionStore {
    pub fn local_secret_store(&self) -> Result<LocalSecretStore> {
        let directory = self.directory().ok_or(Error::Denied(
            "persistent state directory required for local SecretStore",
        ))?;
        // AW-P6-10L: SQLite VACUUM snapshots DO NOT contain the separate
        // owner-only encrypted SecretStore master key or write-once blobs.
        // If a restored SQLite v33 archive has EVER recorded local encrypted
        // custody, do not silently initialize a NEW, unrelated master key
        // when none was restored. This bounded SQL check is needed only on
        // the missing-master recovery path; normal vault access is unchanged.
        if !directory.join("secrets/master.key").exists() {
            let c = self.connection()?;
            let referenced: bool = db(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM audit
                               WHERE action IN ('actor.queue','extension.secret','extension.secret.custody')
                               LIMIT 1)
                     OR EXISTS(SELECT 1 FROM provider_secret_versions LIMIT 1)
                     OR EXISTS(SELECT 1 FROM provider_accounts WHERE secret_ref IS NOT NULL LIMIT 1)
                     OR EXISTS(SELECT 1 FROM oidc_provider_secret_versions LIMIT 1)
                     OR EXISTS(SELECT 1 FROM oauth_connector_client_secret_versions LIMIT 1)
                     OR EXISTS(SELECT 1 FROM oauth_connector_token_versions LIMIT 1)",
                [],
                |row| row.get(0),
            ))?;
            if referenced {
                return Err(Error::Denied(
                    "SQLite snapshot references separate encrypted SecretStore custody; restore matching original master key and items before opening vault",
                ));
            }
        }
        LocalSecretStore::open(directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_never_exposes_wrong_ref() {
        let store = MemorySecretStore::default();
        let a = provider_secret_ref("store_a", "account", 1).unwrap();
        let b = provider_secret_ref("store_a", "account", 2).unwrap();
        store.put(&a, b"top-secret").unwrap();
        assert_eq!(store.get(&a).unwrap().as_slice(), b"top-secret");
        assert!(store.get(&b).is_err());
        store.delete(&a).unwrap();
        assert!(!store.exists(&a).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn local_store_encrypts_private_write_once_blobs() {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        let root = std::env::temp_dir().join(format!(
            "aw-secret-store-test-{}",
            random_bytes::<8>()
                .unwrap()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(&root).unwrap();
        let store = LocalSecretStore::open(&root).unwrap();
        let secret_ref = provider_secret_ref("store_fixture", "shared", 1).unwrap();
        store.put(&secret_ref, b"plaintext-provider-key").unwrap();
        assert_eq!(
            store.get(&secret_ref).unwrap().as_slice(),
            b"plaintext-provider-key"
        );
        store.put(&secret_ref, b"plaintext-provider-key").unwrap();
        assert!(store.put(&secret_ref, b"replacement").is_err());
        let master = root.join("secrets/master.key");
        let blob = root.join("secrets/items").join(format!("{secret_ref}.bin"));
        let master_meta = std::fs::metadata(&master).unwrap();
        let blob_meta = std::fs::metadata(&blob).unwrap();
        assert_eq!(master_meta.mode() & 0o077, 0);
        assert_eq!(blob_meta.mode() & 0o077, 0);
        let raw = std::fs::read(&blob).unwrap();
        assert!(!raw
            .windows(b"plaintext-provider-key".len())
            .any(|w| w == b"plaintext-provider-key"));
        assert!(store.exists(&secret_ref).unwrap());
        std::fs::remove_file(&master).unwrap();
        assert!(LocalSecretStore::open(&root).is_err());
        // Existing in-memory handle can still revoke/delete its active blob.
        store.delete(&secret_ref).unwrap();
        assert!(!store.exists(&secret_ref).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn missing_master_key_never_overwrites_recoverable_secret_state() {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!(
            "aw-secret-missing-key-{}",
            random_bytes::<8>()
                .unwrap()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(&root).unwrap();
        let store = LocalSecretStore::open(&root).unwrap();
        let secret_ref = provider_secret_ref("store_fixture", "shared", 1).unwrap();
        store.put(&secret_ref, b"recoverable-secret").unwrap();
        std::fs::remove_file(root.join("secrets/master.key")).unwrap();
        assert!(LocalSecretStore::open(&root).is_err());
        assert!(!root.join("secrets/master.key").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn secret_refs_are_store_account_and_version_scoped() {
        assert_ne!(
            provider_secret_ref("store_a", "account", 1).unwrap(),
            provider_secret_ref("store_b", "account", 1).unwrap()
        );
        assert_ne!(
            provider_secret_ref("store_a", "account", 1).unwrap(),
            provider_secret_ref("store_a", "account", 2).unwrap()
        );
    }
}
