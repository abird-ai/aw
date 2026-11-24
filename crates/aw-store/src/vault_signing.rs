//! A concrete, trusted local signing broker. Dedicated keys live in the
//! provider's SecretStore, not Agent state, inference custody or tool output.
//! Only a domain-separated HMAC operation is exposed; never a key read.
use crate::{
    id, user_for_principal, Error, LocalSecretStore, Result, SecretStore, SessionStore,
    VaultGrantAuthority, VaultGrantPolicy, VaultGrantTarget,
};
use aw_agent_core::{
    DriverKey, PrincipalId, SourceName, VaultProjection, VaultProviderHandle,
    VaultProviderRegistry, VaultProviderRegistryReader,
};
use ring::hmac;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::RwLock};

struct Entry {
    policy: VaultGrantPolicy,
    manager: PrincipalId,
    manager_user: String,
    key_ref: String,
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

impl<S: SecretStore + 'static> SigningVaultProvider<S> {
    pub fn new(provider: DriverKey, keys: S) -> Result<Self> {
        provider.validate()?;
        Ok(Self {
            provider,
            authority_ref: id("vault-authority")?,
            keys,
            state: RwLock::new(State::default()),
        })
    }

    pub fn register(self, registry: &VaultProviderRegistry) -> Result<VaultProviderHandle<Self>> {
        registry.register(self.provider.clone(), vec![VaultProjection::Broker], self)
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
        if target.entry.provider != self.provider
            || target.projection != VaultProjection::Broker
            || !(32..=128).contains(&key.len())
        {
            return Err(Error::Denied(
                "signing Vault requires a dedicated 32..128 byte broker key",
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
        if !state.entries.contains_key(&target.entry.entry) && state.entries.len() >= 1024 {
            return Err(Error::Limit("too many signing Vault entries"));
        }
        let policy = store.vault_policy(target, self.authority_ref.clone(), generation)?;
        let manager_user = {
            let connection = store.connection()?;
            user_for_principal(&connection, manager)?
                .ok_or(Error::Denied("Vault provider manager unavailable"))?
        };
        let mut hash = Sha256::new();
        hash.update(b"aw-vault-signing-key-v1\0");
        hash.update(self.authority_ref.as_bytes());
        hash.update(generation.to_be_bytes());
        let key_ref = format!("sec_{:x}", hash.finalize());
        self.keys.put(&key_ref, key)?;
        let name = policy.target.entry.entry.clone();
        let previous = state.entries.insert(
            name,
            Entry {
                policy,
                manager: manager.clone(),
                manager_user,
                key_ref,
            },
        );
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
        if let Some(entry) = removed {
            self.keys.delete(&entry.key_ref)?;
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
                let key = provider.keys.get(&authority.entry.key_ref)?;
                let signing_key = hmac::Key::new(hmac::HMAC_SHA256, &key);
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
    use aw_agent_core::VaultEntryRef;
    use rusqlite::Connection;
    use std::path::PathBuf;

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
            .require_projection(&target.entry, VaultProjection::Env)
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
