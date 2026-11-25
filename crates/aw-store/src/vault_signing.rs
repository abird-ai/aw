//! A concrete, trusted local signing broker. Dedicated keys live in the
//! provider's SecretStore, not Agent state, inference custody or tool output.
//! Only a domain-separated HMAC operation is exposed; never a key read.
use crate::{
    id, user_for_principal, Error, LocalSecretStore, Result, SandboxEffectAuthority,
    SandboxEffectLaunch, SecretStore, SessionStore, VaultGrantAuthority, VaultGrantPolicy,
    VaultGrantTarget,
};
use aw_agent_core::{
    DriverKey, PrincipalId, SourceName, VaultEntryRef, VaultProjection, VaultProviderHandle,
    VaultProviderRegistry, VaultProviderRegistryReader,
};
use ring::hmac;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::RwLock,
};

const MAX_ENTRIES: usize = 1024;
const MAX_CONFIG_BYTES: usize = 1024 * 1024;

// Host-only provider configuration; no secret bytes or model-selected paths.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SigningConfig {
    version: u16,
    provider: DriverKey,
    entries: Vec<ConfiguredEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfiguredEntry {
    entry: String,
    key_ref: String,
    agent_ref: String,
    name: SourceName,
    projection: VaultProjection,
    scope: String,
    actor: PrincipalId,
    manager: PrincipalId,
}

struct Entry {
    policy: VaultGrantPolicy,
    manager: PrincipalId,
    manager_user: String,
    key_ref: String,
    key_digest: [u8; 32],
}

#[derive(Default)]
struct State {
    generation: u64,
    entries: BTreeMap<String, Entry>,
}

/// Explicitly provisioned host signing service, using the existing encrypted
/// SecretStore implementation (a dedicated provider-owned root in production).
/// Provider configuration/rotation is trusted host work, not an Agent API.
/// A fresh provider instance has a fresh authority identity; saved grants
/// cannot silently revive after provider restart.
pub struct SigningVaultProvider<S = LocalSecretStore> {
    provider: DriverKey,
    authority_ref: String,
    keys: S,
    managed_keys: bool,
    state: RwLock<State>,
}

struct Authority<'a> {
    entry: &'a Entry,
}

impl VaultGrantAuthority for Authority<'_> {
    fn policy(&self) -> &VaultGrantPolicy {
        &self.entry.policy
    }
    fn authorize_attach(&self, manager: &PrincipalId, manager_user: &str) -> Result<()> {
        if &self.entry.manager != manager || self.entry.manager_user != manager_user {
            return Err(Error::Denied("Vault provider attach authority unavailable"));
        }
        Ok(())
    }
    fn authorize_use(&self) -> Result<()> {
        // Entry borrowed under the provider's read guard. Withdrawal and
        // replacement require its exclusive write guard.
        Ok(())
    }
}

impl SigningVaultProvider<LocalSecretStore> {
    /// Explicit trusted host activation from a dedicated, existing provider
    /// directory. `signing.json` selects exact entries, actors, managers and
    /// immutable SecretStore key versions; no key material in configuration.
    ///
    /// The configuration owner must protect this directory and its ancestors
    /// from Agent access and external writers. Editing configuration is not a
    /// live revocation: withdraw the live provider decision first, then update
    /// configuration and explicitly activate a new provider. Restart always
    /// creates a new authority identity; previous Agent grants stay stale.
    pub fn open_configured(store: &SessionStore, root: &Path) -> Result<Self> {
        if store.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "configured signing Vault requires an exclusive Store owner",
            ));
        }
        let state = store.directory().ok_or(Error::Denied(
            "configured signing Vault requires durable state",
        ))?;
        if !root.is_absolute() || root.canonicalize()? != root {
            return Err(Error::Denied(
                "configured signing Vault requires its exact canonical provider directory",
            ));
        }
        if root.starts_with(state) || state.starts_with(root) {
            return Err(Error::Denied(
                "signing Vault requires separate provider key custody",
            ));
        }
        aw_platform::validate_private_dir(root)?;
        let config: SigningConfig = serde_json::from_slice(
            &crate::secret_store::read_private_file(&root.join("signing.json"), MAX_CONFIG_BYTES)?,
        )
        .map_err(|_| Error::Config("invalid signing Vault configuration".into()))?;
        if config.version != 1 || config.entries.len() > MAX_ENTRIES {
            return Err(Error::Config(
                "unsupported or oversized signing Vault configuration".into(),
            ));
        }
        let keys = LocalSecretStore::open_existing(root)?;
        let mut provider = Self::new(config.provider, keys)?;
        provider.managed_keys = false;
        let mut state = State::default();
        let mut key_refs = BTreeSet::new();
        for entry in config.entries {
            if state.entries.contains_key(&entry.entry) || !key_refs.insert(entry.key_ref.clone()) {
                return Err(Error::Conflict(
                    "duplicate configured signing entry or key version",
                ));
            }
            let key = provider.keys.get(&entry.key_ref)?;
            let target = VaultGrantTarget {
                agent_ref: entry.agent_ref,
                name: entry.name,
                entry: VaultEntryRef {
                    provider: provider.provider.clone(),
                    entry: entry.entry,
                },
                projection: entry.projection,
                scope: entry.scope,
                actor: entry.actor,
            };
            state.generation += 1;
            let prepared = provider.prepare_entry(
                store,
                &entry.manager,
                target,
                state.generation,
                entry.key_ref,
                &key,
            )?;
            state
                .entries
                .insert(prepared.policy.target.entry.entry.clone(), prepared);
        }
        provider.state = RwLock::new(state);
        Ok(provider)
    }
}

impl<S: SecretStore + 'static> SigningVaultProvider<S> {
    pub fn new(provider: DriverKey, keys: S) -> Result<Self> {
        provider.validate()?;
        Ok(Self {
            provider,
            authority_ref: id("vault-authority")?,
            keys,
            managed_keys: true,
            state: RwLock::new(State::default()),
        })
    }

    pub fn register(self, registry: &VaultProviderRegistry) -> Result<VaultProviderHandle<Self>> {
        registry.register(
            self.provider.clone(),
            vec![VaultProjection::Broker, VaultProjection::Env],
            self,
        )
    }

    fn prepare_entry(
        &self,
        store: &SessionStore,
        manager: &PrincipalId,
        target: VaultGrantTarget,
        generation: u64,
        key_ref: String,
        key: &[u8],
    ) -> Result<Entry> {
        if target.entry.provider != self.provider
            || !matches!(
                target.projection,
                VaultProjection::Broker | VaultProjection::Env
            )
            || !(32..=128).contains(&key.len())
        {
            return Err(Error::Denied(
                "signing Vault requires a dedicated 32..128 byte signing key",
            ));
        }
        let policy = store.vault_policy(target, self.authority_ref.clone(), generation)?;
        let manager_user = {
            let connection = store.connection()?;
            user_for_principal(&connection, manager)?
                .ok_or(Error::Denied("Vault provider manager unavailable"))?
        };
        Ok(Entry {
            policy,
            manager: manager.clone(),
            manager_user,
            key_ref,
            key_digest: Sha256::digest(key).into(),
        })
    }

    // The provider owns this exact version. A deleted/reused backend ref may
    // not silently rotate an already authorized signing key.
    fn signing_key(&self, entry: &Entry) -> Result<hmac::Key> {
        let key = self.keys.get(&entry.key_ref)?;
        let digest: [u8; 32] = Sha256::digest(&key).into();
        if digest != entry.key_digest {
            return Err(Error::Denied("Vault signing key version changed"));
        }
        Ok(hmac::Key::new(hmac::HMAC_SHA256, &key))
    }

    /// Install or rotate a dedicated signing key and exact grant ceiling.
    /// The host owns provisioning; neither entry nor key bytes are accepted
    /// by Agent configuration. Old keys are removed only from this backend.
    /// If physical removal fails, the new decision still governs future use.
    pub fn provision(
        &self,
        store: &SessionStore,
        manager: &PrincipalId,
        target: VaultGrantTarget,
        key: &[u8],
    ) -> Result<()> {
        if !self.managed_keys {
            return Err(Error::Denied(
                "configured signing keys remain with their configuration owner",
            ));
        }
        let mut state = self
            .state
            .write()
            .map_err(|_| Error::Storage("Vault authority poisoned"))?;
        let generation = state
            .generation
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or(Error::Limit("Vault authority generation exhausted"))?;
        if !state.entries.contains_key(&target.entry.entry) && state.entries.len() >= MAX_ENTRIES {
            return Err(Error::Limit("too many signing Vault entries"));
        }
        let mut hash = Sha256::new();
        hash.update(b"aw-vault-signing-key-v1\0");
        hash.update(self.authority_ref.as_bytes());
        hash.update(generation.to_be_bytes());
        let key_ref = format!("sec_{:x}", hash.finalize());
        let entry = self.prepare_entry(store, manager, target, generation, key_ref, key)?;
        self.keys.put(&entry.key_ref, key)?;
        let name = entry.policy.target.entry.entry.clone();
        let previous = state.entries.insert(name, entry);
        state.generation = generation;
        drop(state);
        if let Some(previous) = previous {
            self.keys.delete(&previous.key_ref)?;
        }
        Ok(())
    }

    /// Explicit provider-side withdrawal, independent of Agent revision.
    /// Future broker use fails closed; no retroactive erasure claim.
    pub fn revoke(&self, entry: &str) -> Result<()> {
        let removed = self
            .state
            .write()
            .map_err(|_| Error::Storage("Vault authority poisoned"))?
            .entries
            .remove(entry);
        if self.managed_keys {
            if let Some(entry) = removed {
                self.keys.delete(&entry.key_ref)?;
            }
        }
        Ok(())
    }

    fn with_entry<R>(&self, entry: &str, f: impl FnOnce(Authority<'_>) -> Result<R>) -> Result<R> {
        let state = self
            .state
            .read()
            .map_err(|_| Error::Storage("Vault authority poisoned"))?;
        let entry = state
            .entries
            .get(entry)
            .ok_or(Error::Denied("Vault entry unavailable"))?;
        // Actual provider guard retained until the Store transaction and
        // operation return, not a cloned decision detached from authority.
        f(Authority { entry })
    }
}

/// Trusted host selects the exact named entry and provider-owned scope.
/// The challenge is a bounded, application-selected nonce or request.
#[derive(Clone, Debug)]
pub struct SigningProcessProof {
    pub name: SourceName,
    pub entry: String,
    pub scope: String,
    pub grant_rev: u64,
    pub challenge: Vec<u8>,
}

/// Provider-owned integration point. Neither the Agent nor the grant may
/// select another environment name or obtain the actual signing key.
pub const SIGNING_PROOF_ENV: &str = "AW_VAULT_SIGNING_PROOF";

impl<S: SecretStore + 'static> SigningVaultProvider<S> {
    /// Materialize only a domain-separated, exact-Process-bound proof. The
    /// provider guard and original Store/Tool/Sandbox boundary remain held
    /// while the trusted driver hands the proof to actual synchronous spawn.
    /// A materialized proof cannot be retroactively revoked.
    pub fn with_process_proof<R>(
        registry: &VaultProviderRegistryReader,
        handle: &VaultProviderHandle<Self>,
        launch: SandboxEffectLaunch,
        source: &impl SandboxEffectAuthority,
        selection: SigningProcessProof,
        project: impl FnOnce(&[u8; 32]) -> Result<R>,
    ) -> Result<R> {
        if selection.challenge.len() > 64 * 1024 || selection.challenge.is_empty() {
            return Err(Error::Limit(
                "Vault Process proof requires 1..65536 challenge bytes",
            ));
        }
        let provider = registry.get(handle)?;
        provider.with_entry(&selection.entry, |authority| {
            let target = &authority.policy().target;
            if target.name != selection.name
                || target.scope != selection.scope
                || target.projection != VaultProjection::Env
            {
                return Err(Error::Denied(
                    "Vault Process proof scope or projection changed",
                ));
            }
            let effect = launch.effect_id().to_owned();
            launch.with_vault_projection(source, registry, &authority, selection.grant_rev, || {
                let signing_key = provider.signing_key(authority.entry)?;
                let mut mac = hmac::Context::with_key(&signing_key);
                mac.update(b"aw-vault-process-proof-v1\0");
                for part in [
                    authority.policy().store_id().as_bytes(),
                    &authority.policy().restore_generation().to_be_bytes(),
                    target.agent_ref.as_bytes(),
                    target.actor.as_str().as_bytes(),
                    target.name.as_str().as_bytes(),
                    target.entry.entry.as_bytes(),
                    target.scope.as_bytes(),
                    effect.as_bytes(),
                    &selection.grant_rev.to_be_bytes(),
                    &selection.challenge,
                ] {
                    mac.update(&(part.len() as u64).to_be_bytes());
                    mac.update(part);
                }
                let proof = mac.sign();
                project(proof.as_ref().try_into().expect("HMAC-SHA256 tag length"))
            })
        })
    }
}

impl SessionStore {
    /// Trusted host composition, not an RPC. The actual registered provider
    /// supplies its independent exact attachment decision.
    pub fn agent_vault_attach_signing<S: SecretStore + 'static>(
        &self,
        manager: &PrincipalId,
        reference: &str,
        name: &SourceName,
        expected_rev: u64,
        registry: &VaultProviderRegistryReader,
        handle: &VaultProviderHandle<SigningVaultProvider<S>>,
        entry: &str,
    ) -> Result<u64> {
        let provider = registry.get(handle)?;
        provider.with_entry(entry, |authority| {
            provider.signing_key(authority.entry)?;
            self.agent_vault_set(
                manager,
                reference,
                name,
                expected_rev,
                Some((registry, &authority)),
            )
        })
    }

    /// Provider-owned service operation at the real synchronous broker
    /// boundary. No raw secret, generic decrypt, arbitrary target or new
    /// execution route. The scope and exact Agent binding are independently
    /// selected by the trusted host. Tag = HMAC-SHA256(domain || len(scope)
    /// || scope || len(message) || message), lengths unsigned BE64.
    pub fn vault_sign<S: SecretStore + 'static>(
        &self,
        actor: &PrincipalId,
        reference: &str,
        name: &SourceName,
        expected_grant_rev: u64,
        scope: &str,
        registry: &VaultProviderRegistryReader,
        handle: &VaultProviderHandle<SigningVaultProvider<S>>,
        entry: &str,
        message: &[u8],
    ) -> Result<[u8; 32]> {
        if message.len() > 64 * 1024 {
            return Err(Error::Limit("Vault signing message exceeds 64 KiB"));
        }
        let provider = registry.get(handle)?;
        provider.with_entry(entry, |authority| {
            let target = &authority.policy().target;
            if target.agent_ref != reference
                || &target.name != name
                || target.scope != scope
                || &target.actor != actor
            {
                return Err(Error::Denied("Vault signing scope or subject changed"));
            }
            self.with_vault_broker(actor, registry, &authority, expected_grant_rev, || {
                let signing_key = provider.signing_key(authority.entry)?;
                let mut mac = hmac::Context::with_key(&signing_key);
                mac.update(b"aw-vault-signing-broker-v1\0");
                mac.update(&(scope.len() as u64).to_be_bytes());
                mac.update(scope.as_bytes());
                mac.update(&(message.len() as u64).to_be_bytes());
                mac.update(message);
                Ok(mac
                    .sign()
                    .as_ref()
                    .try_into()
                    .expect("HMAC-SHA256 tag length"))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent_owner::{self, AgentKey},
        AgentAdmission, AgentHistoryForkAccess, AgentPermissions, CheckpointRestoreRequest,
        SessionBinding, VaultGrantStatus,
    };
    use rusqlite::Connection;
    use std::path::PathBuf;

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn configured_signing_provider_reopens_existing_keys_without_reviving_grants() {
        use std::io::Write;

        let temp = Temp(std::env::temp_dir().join(id("aw-configured-vault").unwrap()));
        aw_platform::ensure_private_dir(&temp.0).unwrap();
        let state = temp.0.join("state");
        let root = temp.0.canonicalize().unwrap().join("provider");
        let workspace = temp.0.join("workspace");
        aw_platform::ensure_private_dir(&root).unwrap();
        aw_platform::ensure_private_dir(&workspace).unwrap();
        let store = SessionStore::open(&state).unwrap();
        let actor = PrincipalId::new("internal:fixture:configured-signing").unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "signing",
                SessionBinding {
                    working_directory: workspace.clone(),
                    tool_root: workspace,
                    system_prompt: "fixture".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "fixture".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let reference = agent_owner::canonical_ref(
            &store.connection().unwrap(),
            AgentKey::new(&session.id, &branch.id),
        )
        .unwrap();
        let provider_key = DriverKey::builtin("configured-signing").unwrap();
        let name = SourceName::new("signing").unwrap();
        let key_ref = format!("sec_{}", "1".repeat(64));
        let entry = "dedicated-signing";
        let scope = "configured-proof";
        let entry_config = serde_json::json!({
            "entry": entry, "key_ref": key_ref, "agent_ref": reference,
            "name": name, "projection": "broker", "scope": scope,
            "actor": actor, "manager": actor,
        });
        let config = |entries| serde_json::json!({"version": 1, "provider": provider_key, "entries": entries});
        let config_path = root.join("signing.json");
        let mut file = aw_platform::open_private_new(&config_path).unwrap();
        file.write_all(&serde_json::to_vec(&config(vec![entry_config.clone()])).unwrap())
            .unwrap();
        drop(file);
        assert!(SigningVaultProvider::open_configured(&store, &root).is_err());
        assert!(
            !root.join("secrets").exists(),
            "activation must not initialize a backend"
        );
        assert!(SigningVaultProvider::open_configured(&store, store.directory().unwrap()).is_err());

        let keys = LocalSecretStore::open(&root).unwrap();
        let original_key = [41u8; 32];
        keys.put(&key_ref, &original_key).unwrap();
        // A malformed/duplicate set cannot produce a partly registered provider.
        std::fs::write(
            &config_path,
            serde_json::to_vec(&config(vec![entry_config.clone(), entry_config.clone()])).unwrap(),
        )
        .unwrap();
        assert!(SigningVaultProvider::open_configured(&store, &root).is_err());
        std::fs::write(
            &config_path,
            serde_json::to_vec(&config(vec![entry_config])).unwrap(),
        )
        .unwrap();
        let registry = VaultProviderRegistry::new();
        let handle = SigningVaultProvider::open_configured(&store, &root)
            .unwrap()
            .register(&registry)
            .unwrap();
        let reader = registry.reader().unwrap();
        let provider = reader.get(&handle).unwrap();
        let attach = |store: &SessionStore,
                      reader: &VaultProviderRegistryReader,
                      handle: &VaultProviderHandle<SigningVaultProvider>,
                      rev| {
            store.agent_vault_attach_signing(&actor, &reference, &name, rev, reader, handle, entry)
        };
        let sign = |store: &SessionStore,
                    reader: &VaultProviderRegistryReader,
                    handle: &VaultProviderHandle<SigningVaultProvider>,
                    rev| {
            store.vault_sign(
                &actor,
                &reference,
                &name,
                rev,
                scope,
                reader,
                handle,
                entry,
                b"challenge",
            )
        };
        let original_rev = store.agent_get(&actor, &reference).unwrap().rev;
        assert!(sign(&store, &reader, &handle, original_rev).is_err());
        let rev = attach(&store, &reader, &handle, original_rev).unwrap();
        let proof = sign(&store, &reader, &handle, rev).unwrap();
        let target = VaultGrantTarget {
            agent_ref: reference.clone(),
            name: name.clone(),
            entry: VaultEntryRef {
                provider: provider_key,
                entry: entry.into(),
            },
            projection: VaultProjection::Broker,
            scope: scope.into(),
            actor: actor.clone(),
        };
        assert!(provider
            .provision(&store, &actor, target, &[99; 32])
            .is_err());

        // The original version cannot be silently replaced under a live grant.
        keys.delete(&key_ref).unwrap();
        keys.put(&key_ref, &[73; 32]).unwrap();
        assert!(sign(&store, &reader, &handle, rev).is_err());
        assert!(attach(&store, &reader, &handle, rev).is_err());
        assert_eq!(store.agent_get(&actor, &reference).unwrap().rev, rev);
        provider.revoke(entry).unwrap();
        assert_eq!(keys.get(&key_ref).unwrap().as_slice(), &[73; 32]);
        assert!(sign(&store, &reader, &handle, rev).is_err());
        drop(store);

        // A trusted explicit reactivation loads a fresh authority, not a saved
        // grant. The persisted key remains under its configuration owner.
        let store = SessionStore::open(&state).unwrap();
        let restarted = VaultProviderRegistry::new();
        let restarted_handle = SigningVaultProvider::open_configured(&store, &root)
            .unwrap()
            .register(&restarted)
            .unwrap();
        let restarted_reader = restarted.reader().unwrap();
        assert!(sign(&store, &restarted_reader, &restarted_handle, rev).is_err());
        let new_rev = attach(&store, &restarted_reader, &restarted_handle, rev).unwrap();
        assert_eq!(new_rev, rev + 1);
        assert_ne!(
            sign(&store, &restarted_reader, &restarted_handle, new_rev).unwrap(),
            proof
        );
        assert!(!crate::json(
            &store
                .agent_vault_list(&actor, &reference, |_, _| Ok(()))
                .unwrap()
        )
        .unwrap()
        .contains(&key_ref));
    }

    #[test]
    fn signing_vault_uses_exact_agent_grants_and_live_provider_authority() {
        let temp = Temp(std::env::temp_dir().join(id("aw-vault").unwrap()));
        aw_platform::ensure_private_dir(&temp.0).unwrap();
        let state = temp.0.join("state");
        let keys_root = temp.0.join("provider");
        aw_platform::ensure_private_dir(&keys_root).unwrap();
        let store = SessionStore::open(&state).unwrap();
        let owner = PrincipalId::new("internal:fixture:vault-owner").unwrap();
        let peer = PrincipalId::new("internal:fixture:vault-peer").unwrap();
        let layout = SessionBinding {
            working_directory: temp.0.clone(),
            tool_root: temp.0.clone(),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (session, branch) = store.create(&owner, "signing", layout.clone()).unwrap();
        let (other, other_branch) = store
            .create(&owner, "shared layout", layout.clone())
            .unwrap();
        let (reference, other_ref, peer_ref) = {
            let connection = store.connection().unwrap();
            let reference =
                agent_owner::canonical_ref(&connection, AgentKey::new(&session.id, &branch.id))
                    .unwrap();
            let other_ref =
                agent_owner::canonical_ref(&connection, AgentKey::new(&other.id, &other_branch.id))
                    .unwrap();
            let peer_user = crate::ensure_user_for_principal(&connection, &peer).unwrap();
            let domain = reference.split("/i:").next().unwrap();
            let peer_ref = format!("{domain}/i:{peer_user}");
            (reference, other_ref, peer_ref)
        };
        let original = store.agent_get(&owner, &reference).unwrap();
        assert_eq!(
            original.sandbox_ref,
            store.agent_get(&owner, &other_ref).unwrap().sandbox_ref
        );
        drop(store);
        // Populated v58 -> v59. No legacy Agent or shared Sandbox is
        // assigned any implicit Vault grant during migration.
        let database = state.join("sessions.sqlite3");
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "DROP TABLE agent_vault_grants_current;
             UPDATE store_meta SET value='58' WHERE key='schema_epoch';
             UPDATE schema_meta SET schema_epoch=58,activation_epoch=58 WHERE singleton=1;
             UPDATE migration_lease SET current_epoch=58 WHERE singleton=1;
             PRAGMA user_version=58;",
            )
            .unwrap();
        let store = SessionStore::open(&state).unwrap();
        assert_eq!(store.agent_get(&owner, &reference).unwrap(), original);
        assert!(store
            .agent_vault_list(&owner, &reference, |_, _| Ok(()))
            .unwrap()
            .is_empty());

        let keys = LocalSecretStore::open(&keys_root).unwrap();
        let provider_key = DriverKey::builtin("local-signing").unwrap();
        let registry = VaultProviderRegistry::new();
        let handle = SigningVaultProvider::new(provider_key.clone(), keys.clone())
            .unwrap()
            .register(&registry)
            .unwrap();
        let reader = registry.reader().unwrap();
        let provider = reader.get(&handle).unwrap();
        assert!(VaultProviderRegistry::new()
            .reader()
            .unwrap()
            .get(&handle)
            .is_err());
        assert!(registry
            .register(provider_key.clone(), vec![VaultProjection::Broker], ())
            .is_err());
        let name = SourceName::new("signing").unwrap();
        let target = VaultGrantTarget {
            agent_ref: reference.clone(),
            name: name.clone(),
            entry: VaultEntryRef {
                provider: provider_key.clone(),
                entry: "fixture-signing".into(),
            },
            projection: VaultProjection::Broker,
            scope: "fixture-proof".into(),
            actor: owner.clone(),
        };
        let key = [41u8; 32];
        provider
            .provision(&store, &owner, target.clone(), &key)
            .unwrap();
        let attach = |store: &SessionStore, manager: &PrincipalId, rev| {
            store.agent_vault_attach_signing(
                manager,
                &reference,
                &name,
                rev,
                &reader,
                &handle,
                &target.entry.entry,
            )
        };
        let sign =
            |store: &SessionStore, actor: &PrincipalId, reference: &str, rev, scope: &str| {
                store.vault_sign(
                    actor,
                    reference,
                    &name,
                    rev,
                    scope,
                    &reader,
                    &handle,
                    &target.entry.entry,
                    b"challenge",
                )
            };
        let expected = |key: &[u8]| {
            let mut payload = b"aw-vault-signing-broker-v1\0".to_vec();
            payload.extend_from_slice(&(target.scope.len() as u64).to_be_bytes());
            payload.extend_from_slice(target.scope.as_bytes());
            payload.extend_from_slice(&9u64.to_be_bytes());
            payload.extend_from_slice(b"challenge");
            let key = hmac::Key::new(hmac::HMAC_SHA256, key);
            <[u8; 32]>::try_from(hmac::sign(&key, &payload).as_ref()).unwrap()
        };
        // Registration and provisioning alone do not grant use.
        assert!(sign(&store, &owner, &reference, original.rev, &target.scope).is_err());
        let rev = attach(&store, &owner, original.rev).unwrap();
        assert_eq!(rev, original.rev + 1);
        assert_eq!(attach(&store, &owner, rev).unwrap(), rev);
        assert!(attach(&store, &owner, original.rev).is_err());
        assert_eq!(
            sign(&store, &owner, &reference, rev, &target.scope).unwrap(),
            expected(&key)
        );
        assert!(sign(&store, &owner, &other_ref, rev, &target.scope).is_err());
        assert!(sign(&store, &owner, &reference, rev, "wrong-scope").is_err());
        assert!(reader
            .require_projection(&target.entry, VaultProjection::Socket)
            .is_err());
        assert!(store
            .agent_vault_list::<()>(&owner, &reference, |_, _| Err(Error::Denied(
                "no metadata authority"
            )))
            .is_err());
        let summary = store
            .agent_vault_list(&owner, &reference, |_, _| Ok(()))
            .unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].grant_rev, rev);
        assert_eq!(summary[0].status, VaultGrantStatus::RevalidationRequired);
        assert!(!crate::json(&summary).unwrap().contains("fixture-signing"));

        // Agent ACL broadening does not grant the provider entry to the
        // newly admitted participant, even with Manage. Explicit regrant
        // is required for the original actor after access-generation change.
        let broadened = store
            .agent_access_set_ref(
                &owner,
                &reference,
                &peer_ref,
                Some(AgentPermissions::ALL),
                rev,
            )
            .unwrap();
        assert!(sign(&store, &owner, &reference, rev, &target.scope).is_err());
        assert!(attach(&store, &peer, broadened.rev).is_err());
        assert!(sign(&store, &peer, &reference, rev, &target.scope).is_err());
        assert_eq!(
            store
                .agent_vault_list(&owner, &reference, |_, _| Ok(()))
                .unwrap()[0]
                .status,
            VaultGrantStatus::Stale
        );
        let rev = attach(&store, &owner, broadened.rev).unwrap();
        assert_eq!(rev, broadened.rev + 1);
        assert_eq!(
            sign(&store, &owner, &reference, rev, &target.scope).unwrap(),
            expected(&key)
        );

        // Current authority survives Store reopen, but provider rotation
        // does not update any Agent rows or silently authorize a new key.
        drop(store);
        let store = SessionStore::open(&state).unwrap();
        assert_eq!(
            sign(&store, &owner, &reference, rev, &target.scope).unwrap(),
            expected(&key)
        );
        let rotated_key = [73u8; 32];
        provider
            .provision(&store, &owner, target.clone(), &rotated_key)
            .unwrap();
        assert_eq!(store.agent_get(&owner, &reference).unwrap().rev, rev);
        assert!(sign(&store, &owner, &reference, rev, &target.scope).is_err());
        let rotated_rev = attach(&store, &owner, rev).unwrap();
        assert_eq!(rotated_rev, rev + 1);
        assert!(sign(&store, &owner, &reference, rev, &target.scope).is_err());
        assert_eq!(
            sign(&store, &owner, &reference, rotated_rev, &target.scope).unwrap(),
            expected(&rotated_key)
        );

        // A newly constructed provider instance cannot reinterpret saved
        // authority generations even with the same key and stable entry ID.
        let restarted_registry = VaultProviderRegistry::new();
        let restarted_handle = SigningVaultProvider::new(provider_key, keys)
            .unwrap()
            .register(&restarted_registry)
            .unwrap();
        let restarted_reader = restarted_registry.reader().unwrap();
        let restarted = restarted_reader.get(&restarted_handle).unwrap();
        restarted
            .provision(&store, &owner, target.clone(), &rotated_key)
            .unwrap();
        assert!(store
            .vault_sign(
                &owner,
                &reference,
                &name,
                rotated_rev,
                &target.scope,
                &restarted_reader,
                &restarted_handle,
                &target.entry.entry,
                b"challenge"
            )
            .is_err());
        restarted.revoke(&target.entry.entry).unwrap();

        // History/ACL inheritance and Sandbox sharing do not copy grants.
        let fresh_root = temp.0.join("restored");
        aw_platform::ensure_private_dir(&fresh_root).unwrap();
        let restored_layout = SessionBinding {
            working_directory: fresh_root.clone(),
            tool_root: fresh_root,
            ..layout.clone()
        };
        let checkpoint = store
            .checkpoint_create_history_only(&owner, &reference, None, "fixture:vault-history")
            .unwrap();
        let restored = store
            .checkpoint_restore_baseline(
                &owner,
                &CheckpointRestoreRequest {
                    checkpoint: checkpoint.id,
                    project: None,
                    title: None,
                    access: AgentHistoryForkAccess::Inherit,
                    task: None,
                },
                &restored_layout,
                &[],
                "fixture:vault-restore",
                "local",
            )
            .unwrap();
        assert!(store
            .agent_vault_list(&owner, &restored.agent.canonical_ref, |_, _| Ok(()))
            .unwrap()
            .is_empty());
        assert!(store
            .agent_vault_list(&owner, &other_ref, |_, _| Ok(()))
            .unwrap()
            .is_empty());

        // Held admission is not bypassed by the broker, and provider
        // withdrawal governs future use without an Agent revision fanout.
        let held = store
            .agent_admission(&owner, &reference, AgentAdmission::Held, Some(rotated_rev))
            .unwrap();
        assert!(sign(&store, &owner, &reference, rotated_rev, &target.scope).is_err());
        let open = store
            .agent_admission(&owner, &reference, AgentAdmission::Open, Some(held.rev))
            .unwrap();
        provider.revoke(&target.entry.entry).unwrap();
        assert_eq!(store.agent_get(&owner, &reference).unwrap().rev, open.rev);
        assert!(sign(&store, &owner, &reference, rotated_rev, &target.scope).is_err());
        assert!(std::fs::read_dir(keys_root.join("secrets/items"))
            .unwrap()
            .next()
            .is_none());
        // Removal is allowed after terminal close, but no new attach.
        store.agent_request_close(&owner, &reference, None).unwrap();
        let closed = store.agent_finish_close(&owner, &reference).unwrap();
        provider
            .provision(&store, &owner, target.clone(), &key)
            .unwrap();
        assert!(attach(&store, &owner, closed.rev).is_err());
        let removed = store
            .agent_vault_set(&owner, &reference, &name, closed.rev, None)
            .unwrap();
        assert_eq!(removed, closed.rev + 1);
        assert_eq!(
            store
                .agent_vault_set(&owner, &reference, &name, removed, None)
                .unwrap(),
            removed
        );
        assert!(store
            .agent_vault_list(&owner, &reference, |_, _| Ok(()))
            .unwrap()
            .is_empty());
        provider.revoke(&target.entry.entry).unwrap();
    }
}
