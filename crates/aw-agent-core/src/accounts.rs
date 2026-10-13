//! Identity and credential-placement metadata. NO API keys, tokens, endpoints,
//! cookies or environment-variable names occur in these records.
//!
//! The authenticated host selects the principal. A submitted prompt, model tool
//! call, or browser JSON must never be allowed to choose an arbitrary principal.
use crate::{publication, Error, Provider, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PrincipalId(String);
impl PrincipalId {
    pub fn new(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        valid_id(&id)?;
        Ok(Self(id))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for PrincipalId {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}
fn valid_id(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 128
        || !s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b":._-@".contains(&c))
    {
        return Err(Error::Config(
            "identity/account/executor IDs must be portable 1..128 byte labels".into(),
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AccountOwner {
    User(PrincipalId),
    Server(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum BillingOwner {
    User(PrincipalId),
    Shared(String),
}
/// Custody and inference location are different from the collaboration server.
/// BrowserMemory is a descriptor for a future trusted client adapter; the
/// supplied HTTP provider runs in Rust, never automatically in browser JS.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum CredentialCustody {
    UserDevice(String),
    Server(String),
    BrowserMemory(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountDescriptor {
    pub id: String,
    pub owner: AccountOwner,
    pub payer: BillingOwner,
    pub credential_custody: CredentialCustody,
    /// Secret-free provenance for a versioned server/shared credential. Personal
    /// and legacy environment-backed accounts remain None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_version: Option<u64>,
    /// The executor allowed to use this opaque provider handle. Never an URL.
    pub inference_executor: String,
    pub allowed_models: BTreeSet<String>,
    /// Explicit grant for using a common account; empty means owner-only.
    pub grants: BTreeSet<PrincipalId>,
    /// Only valid for a server-owned common account, never implicit default.
    pub shared_with_all_authenticated: bool,
}
impl AccountDescriptor {
    pub fn personal(
        id: impl Into<String>,
        user: PrincipalId,
        device: impl Into<String>,
        models: impl IntoIterator<Item = String>,
    ) -> Self {
        let device = device.into();
        Self {
            id: id.into(),
            owner: AccountOwner::User(user.clone()),
            payer: BillingOwner::User(user),
            credential_custody: CredentialCustody::UserDevice(device.clone()),
            credential_version: None,
            inference_executor: device,
            allowed_models: models.into_iter().collect(),
            grants: BTreeSet::new(),
            shared_with_all_authenticated: false,
        }
    }
    fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        valid_id(&self.inference_executor)?;
        match &self.owner {
            AccountOwner::User(p) => valid_id(p.as_str())?,
            AccountOwner::Server(id) => valid_id(id)?,
        }
        match &self.payer {
            BillingOwner::User(p) => valid_id(p.as_str())?,
            BillingOwner::Shared(id) => valid_id(id)?,
        }
        match (&self.owner,&self.credential_custody) {
            (AccountOwner::User(_),CredentialCustody::UserDevice(_)|CredentialCustody::BrowserMemory(_))=>{},
            (AccountOwner::Server(_),CredentialCustody::Server(_))=>{},
            _=>return Err(Error::Denied("personal credentials stay on their user executor; server custody must be an explicit server-owned account")),
        }
        if self.credential_version == Some(0) {
            return Err(Error::Config("credential_version must be positive".into()));
        }
        if matches!(self.owner, AccountOwner::User(_)) && self.credential_version.is_some() {
            return Err(Error::Denied(
                "personal account descriptors never carry server credential versions",
            ));
        }
        if self.credential_version.is_some()
            && !matches!(self.credential_custody, CredentialCustody::Server(_))
        {
            return Err(Error::Denied(
                "credential versions are reserved for server SecretStore custody",
            ));
        }
        let site = match &self.credential_custody {
            CredentialCustody::UserDevice(s)
            | CredentialCustody::Server(s)
            | CredentialCustody::BrowserMemory(s) => s,
        };
        valid_id(site)?;
        if site != &self.inference_executor {
            return Err(Error::Denied("credential custody and this executor must match; use a delegated adapter instead of forwarding a key"));
        }
        if self.allowed_models.is_empty() || self.allowed_models.len() > 128 {
            return Err(Error::Config(
                "account needs 1..128 explicitly allowed models".into(),
            ));
        }
        for model in &self.allowed_models {
            if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
                return Err(Error::Config("invalid allowed model".into()));
            }
        }
        if self.grants.len() > 1024 {
            return Err(Error::Limit("too many account grants"));
        }
        for p in &self.grants {
            valid_id(p.as_str())?;
        }
        if let AccountOwner::User(owner) = &self.owner {
            if self.payer != BillingOwner::User(owner.clone()) {
                return Err(Error::Denied("personal account billing must belong to its owner; use an explicit shared account otherwise"));
            }
        }
        if matches!(self.owner, AccountOwner::User(_))
            && (self.shared_with_all_authenticated || !self.grants.is_empty())
        {
            return Err(Error::Denied("personal accounts are owner-only; register an explicit server-owned common account to share"));
        }
        Ok(())
    }
    fn allows(&self, actor: &PrincipalId) -> bool {
        match &self.owner {
            AccountOwner::User(owner) => owner == actor,
            AccountOwner::Server(_) => {
                self.shared_with_all_authenticated || self.grants.contains(actor)
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InvocationIdentity {
    pub actor: PrincipalId,
    pub account_id: String,
    pub account_owner: AccountOwner,
    pub payer: BillingOwner,
    pub credential_custody: CredentialCustody,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_version: Option<u64>,
    pub inference_executor: String,
    pub model: String,
    pub grant_revision: u64,
}
#[derive(Clone)]
struct Entry {
    descriptor: AccountDescriptor,
    provider: Arc<dyn Provider>,
    revision: u64,
}
#[derive(Clone, Default)]
struct PoolState {
    entries: BTreeMap<String, Arc<Entry>>,
    revision: u64,
}
/// Trusted owner-managed registry. Concrete providers stay at their executing
/// host; explicit delegated adapters reference permitted personal devices without
/// importing their provider configuration. No fallback path.
#[derive(Clone)]
pub struct ProviderAccounts {
    executor: String,
    state: Arc<publication::Publisher<PoolState>>,
    readers: publication::ReaderFactory<PoolState>,
}
impl ProviderAccounts {
    pub fn new(executor: impl Into<String>) -> Result<Self> {
        let executor = executor.into();
        valid_id(&executor)?;
        let (publisher, readers) = publication::channel(PoolState::default());
        Ok(Self {
            executor,
            state: Arc::new(publisher),
            readers,
        })
    }
    /// Registration never reads a secret or contacts a provider. Handles may be
    /// lazy. Updating a descriptor invalidates existing selected leases.
    pub fn register(
        &self,
        descriptor: AccountDescriptor,
        provider: Arc<dyn Provider>,
    ) -> Result<()> {
        descriptor.validate()?;
        if descriptor.inference_executor != self.executor || provider.delegated_binding().is_some()
        {
            return Err(Error::Denied("this account belongs to another inference executor; use explicit delegated registration, never forward credentials"));
        }
        self.state
            .update(move |state| {
                if state.entries.len() >= 128 && !state.entries.contains_key(&descriptor.id) {
                    return Err(Error::Limit("account registry full"));
                }
                let revision = state
                    .revision
                    .checked_add(1)
                    .ok_or(Error::Limit("account revision exhausted"))?;
                let mut next = state.clone();
                next.revision = revision;
                next.entries.insert(
                    descriptor.id.clone(),
                    Arc::new(Entry {
                        descriptor,
                        provider,
                        revision,
                    }),
                );
                Ok((next, ()))
            })
            .map_err(registry_error)?;
        Ok(())
    }

    /// Explicit host-only registration of a personal-device adapter. Keeps the
    /// normal local-registration location check intact. Never invoked from JSON.
    pub fn register_delegated(
        &self,
        descriptor: AccountDescriptor,
        provider: Arc<dyn Provider>,
    ) -> Result<()> {
        descriptor.validate()?;
        let binding = provider.delegated_binding().ok_or(Error::Denied(
            "provider is not a declared delegated adapter",
        ))?;
        if !matches!(&descriptor.owner, AccountOwner::User(_))
            || !matches!(
                &descriptor.credential_custody,
                CredentialCustody::UserDevice(_)
            )
            || descriptor.inference_executor == self.executor
            || binding.account_id != descriptor.id
            || binding.executor_id != descriptor.inference_executor
        {
            return Err(Error::Denied(
                "delegated registration does not match this personal device account",
            ));
        }
        self.state
            .update(move |state| {
                if state.entries.contains_key(&descriptor.id) {
                    return Err(Error::Denied(
                        "delegated account cannot replace an existing account",
                    ));
                }
                if state.entries.len() >= 128 {
                    return Err(Error::Limit("account registry full"));
                }
                let revision = state
                    .revision
                    .checked_add(1)
                    .ok_or(Error::Limit("account revision exhausted"))?;
                let mut next = state.clone();
                next.revision = revision;
                next.entries.insert(
                    descriptor.id.clone(),
                    Arc::new(Entry {
                        descriptor,
                        provider,
                        revision,
                    }),
                );
                Ok((next, ()))
            })
            .map_err(registry_error)?;
        Ok(())
    }
    pub fn revoke(&self, account: &str) -> Result<()> {
        self.state
            .update(|state| {
                let mut next = state.clone();
                next.entries.remove(account);
                Ok((next, ()))
            })
            .map_err(registry_error)?;
        Ok(())
    }
    pub fn visible_accounts(&self, actor: &PrincipalId) -> Result<Vec<AccountDescriptor>> {
        let current = self
            .readers
            .reader()
            .map_err(registry_error)?
            .snapshot()
            .map_err(registry_error)?;
        Ok(current
            .value
            .entries
            .values()
            .filter(|entry| entry.descriptor.allows(actor))
            .map(|entry| entry.descriptor.clone())
            .collect())
    }
    /// Call only with an actor established by the host authentication boundary,
    /// not an actor field from an untrusted request. This is capability selection,
    /// not an identity authenticator or a full shared-session ACL service.
    pub fn select(
        &self,
        actor: &PrincipalId,
        account: &str,
        model: &str,
    ) -> Result<ProviderSelection> {
        let current = self
            .readers
            .reader()
            .map_err(registry_error)?
            .snapshot()
            .map_err(registry_error)?;
        let state = &current.value;
        {
            let e = state
                .entries
                .get(account)
                .filter(|e| {
                    e.descriptor.allows(actor) && e.descriptor.allowed_models.contains(model)
                })
                .ok_or(Error::Denied(
                    "provider account or model not permitted; no fallback was used",
                ))?;
            let d = &e.descriptor;
            Ok(ProviderSelection {
                pool: self.clone(),
                provider: e.provider.clone(),
                identity: InvocationIdentity {
                    actor: actor.clone(),
                    account_id: d.id.clone(),
                    account_owner: d.owner.clone(),
                    payer: d.payer.clone(),
                    credential_custody: d.credential_custody.clone(),
                    credential_version: d.credential_version,
                    inference_executor: d.inference_executor.clone(),
                    model: model.into(),
                    grant_revision: e.revision,
                },
            })
        }
    }
}
/// No secret fields and no public constructor. Selected for ONE caller/run;
/// not stored as a shared conversation-global provider preference.
#[derive(Clone)]
pub struct ProviderSelection {
    pool: ProviderAccounts,
    provider: Arc<dyn Provider>,
    identity: InvocationIdentity,
}
impl ProviderSelection {
    pub fn identity(&self) -> &InvocationIdentity {
        &self.identity
    }
    pub(crate) fn provider_arc(&self) -> Arc<dyn Provider> {
        self.provider.clone()
    }
    pub(crate) fn pool(&self) -> ProviderAccounts {
        self.pool.clone()
    }
    pub(crate) fn descriptor(&self) -> Result<AccountDescriptor> {
        self.reader()?.descriptor()
    }
    /// Register once for a task or streaming callback; repeated checks neither
    /// register handles nor mutate a shared snapshot Arc reference count.
    pub fn reader(&self) -> Result<SelectionReader> {
        Ok(SelectionReader {
            reader: self.pool.readers.reader().map_err(registry_error)?,
            identity: self.identity.clone(),
        })
    }
    /// Compatibility convenience for isolated checks. Hot invocation paths own
    /// a SelectionReader instead of registering a new handle for every check.
    pub fn revalidate(&self) -> Result<()> {
        self.reader()?.revalidate()
    }
    pub(crate) fn provider(&self) -> &dyn Provider {
        self.provider.as_ref()
    }
}

/// One task-owned current-grant reader. Send, deliberately !Sync; use one per
/// concurrent invocation/callback and release its internal guard before effects.
pub struct SelectionReader {
    reader: publication::Reader<PoolState>,
    identity: InvocationIdentity,
}
fn registry_error(error: Error) -> Error {
    match error {
        Error::Storage(_) => Error::Denied("provider registry unavailable"),
        error => error,
    }
}
fn validate_selection<'a>(
    state: &'a PoolState,
    identity: &InvocationIdentity,
) -> Result<&'a Entry> {
    validate_entry(
        state.entries.get(&identity.account_id).map(Arc::as_ref),
        identity,
    )
}
fn validate_entry<'a>(
    entry: Option<&'a Entry>,
    identity: &InvocationIdentity,
) -> Result<&'a Entry> {
    let entry = entry.ok_or(Error::Denied("provider grant revoked"))?;
    if entry.revision != identity.grant_revision
        || !entry.descriptor.allows(&identity.actor)
        || !entry.descriptor.allowed_models.contains(&identity.model)
    {
        return Err(Error::Denied(
            "provider grant changed; select again explicitly",
        ));
    }
    Ok(entry)
}
impl SelectionReader {
    pub fn revalidate(&mut self) -> Result<()> {
        self.reader
            .inspect(|state| validate_selection(state, &self.identity).map(|_| ()))
            .map_err(registry_error)?
    }
    fn descriptor(&mut self) -> Result<AccountDescriptor> {
        let current = self.reader.snapshot().map_err(registry_error)?;
        validate_selection(&current.value, &self.identity).map(|entry| entry.descriptor.clone())
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use std::{
        sync::{mpsc, Barrier, RwLock},
        time::{Duration, Instant},
    };

    struct Unused;
    impl Provider for Unused {
        fn stream<'a>(
            &'a self,
            _: &'a [crate::Message],
            _: &'a str,
            _: &'a mut crate::DeltaSink<'_>,
        ) -> crate::CompletionFuture<'a> {
            Box::pin(async { Err(Error::Denied("fixture provider must not run")) })
        }
    }
    fn fixture() -> (
        ProviderAccounts,
        AccountDescriptor,
        PrincipalId,
        ProviderSelection,
    ) {
        let actor = PrincipalId::new("alice").unwrap();
        let descriptor = AccountDescriptor::personal("main", actor.clone(), "local", ["m".into()]);
        let pool = ProviderAccounts::new("local").unwrap();
        pool.register(descriptor.clone(), Arc::new(Unused)).unwrap();
        let selected = pool.select(&actor, "main", "m").unwrap();
        (pool, descriptor, actor, selected)
    }

    // Preserve the original owned Entry map in the RwLock reference, including
    // its original update allocation cost; only the published catalog shares entries.
    struct LegacyState {
        entries: BTreeMap<String, Entry>,
        revision: u64,
    }
    fn legacy_state(pool: &ProviderAccounts) -> LegacyState {
        let current = pool.state.current().unwrap();
        LegacyState {
            entries: current
                .value
                .entries
                .iter()
                .map(|(key, entry)| (key.clone(), entry.as_ref().clone()))
                .collect(),
            revision: current.value.revision,
        }
    }

    #[test]
    fn task_reader_observes_completed_revocation_and_re_registration() {
        let (pool, descriptor, actor, selected) = fixture();
        let mut reader = selected.reader().unwrap();
        let (ready, entered) = mpsc::channel();
        let (check, after) = mpsc::channel();
        let (done, result) = mpsc::channel();
        let task = std::thread::spawn(move || {
            reader.revalidate().unwrap();
            ready.send(()).unwrap();
            after.recv_timeout(Duration::from_secs(5)).unwrap();
            done.send(reader.revalidate().is_err()).unwrap();
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        pool.revoke("main").unwrap();
        assert!(pool.select(&actor, "main", "m").is_err());
        check.send(()).unwrap();
        assert!(result.recv_timeout(Duration::from_secs(5)).unwrap());
        task.join().unwrap();
        pool.register(descriptor, Arc::new(Unused)).unwrap();
        assert!(selected.reader().unwrap().revalidate().is_err());
        let fresh = pool.select(&actor, "main", "m").unwrap();
        assert!(fresh.identity().grant_revision > selected.identity().grant_revision);
        fresh.reader().unwrap().revalidate().unwrap();
    }

    #[test]
    fn failed_registration_and_revision_overflow_preserve_visible_grant() {
        let (pool, mut descriptor, _, selected) = fixture();
        let generation = pool.state.current().unwrap().generation;
        descriptor.inference_executor = "other".into();
        assert!(pool.register(descriptor.clone(), Arc::new(Unused)).is_err());
        assert_eq!(pool.state.current().unwrap().generation, generation);
        selected.reader().unwrap().revalidate().unwrap();
        pool.state
            .update(|old| {
                let mut next = old.clone();
                next.revision = u64::MAX;
                Ok((next, ()))
            })
            .unwrap();
        descriptor.inference_executor = "local".into();
        let generation = pool.state.current().unwrap().generation;
        assert!(matches!(
            pool.register(descriptor, Arc::new(Unused)),
            Err(Error::Limit(_))
        ));
        assert_eq!(pool.state.current().unwrap().generation, generation);
        selected.reader().unwrap().revalidate().unwrap();
    }

    #[test]
    fn writer_panic_denies_existing_readers_and_shared_types_remain_sync() {
        fn shared<T: Send + Sync>() {}
        fn movable<T: Send>() {}
        shared::<ProviderAccounts>();
        shared::<ProviderSelection>();
        movable::<SelectionReader>();
        let (pool, _, actor, selected) = fixture();
        let mut reader = selected.reader().unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = pool
                .state
                .update::<()>(|_| panic!("fixture registry failure"));
        }));
        assert!(panic.is_err());
        assert!(matches!(reader.revalidate(), Err(Error::Denied(_))));
        assert!(matches!(selected.revalidate(), Err(Error::Denied(_))));
        assert!(matches!(
            pool.select(&actor, "main", "m"),
            Err(Error::Denied(_))
        ));
        assert!(matches!(
            pool.visible_accounts(&actor),
            Err(Error::Denied(_))
        ));
    }

    /// Actual bounded account lookups: same revision/actor/model checks, no
    /// providers. Run explicitly in release profile; report cold setup honestly.
    #[test]
    #[ignore = "explicit account contention measurement"]
    fn account_publication_contention_workloads() {
        const READS: usize = 20_000;
        const SAMPLE_STRIDE: usize = 32;
        fn percentiles(mut samples: Vec<u128>) -> [u128; 3] {
            if samples.is_empty() {
                return [0; 3];
            }
            samples.sort_unstable();
            [50, 95, 99].map(|p| samples[(samples.len() - 1) * p / 100])
        }
        for catalog_size in [32, 128] {
            for threads in [1, 2, 4, 8] {
                for per_thousand in [0, 1, 10, 100] {
                    let writes = threads * READS * per_thousand / 1000;
                    let mut timings = Vec::new();
                    for published in [false, true] {
                        let (pool, descriptor, _, selected) = fixture();
                        for index in 1..catalog_size {
                            let mut extra = descriptor.clone();
                            extra.id = if index == 1 {
                                "unrelated".into()
                            } else {
                                format!("account{index}")
                            };
                            pool.register(extra, Arc::new(Unused)).unwrap();
                        }
                        let baseline = Arc::new(RwLock::new(legacy_state(&pool)));
                        let barrier =
                            Arc::new(Barrier::new(threads + 1 + usize::from(writes != 0)));
                        let (observed, samples) = mpsc::channel();
                        let mut tasks = Vec::new();
                        for _ in 0..threads {
                            let selected = selected.clone();
                            let baseline = baseline.clone();
                            let barrier = barrier.clone();
                            let observed = observed.clone();
                            tasks.push(std::thread::spawn(move || {
                                let mut reader = selected.reader().unwrap();
                                let mut latencies = Vec::new();
                                barrier.wait();
                                for index in 0..READS {
                                    let started = (index % SAMPLE_STRIDE == 0).then(Instant::now);
                                    if published {
                                        std::hint::black_box(reader.revalidate()).unwrap();
                                    } else {
                                        std::hint::black_box(validate_entry(
                                            baseline
                                                .read()
                                                .unwrap()
                                                .entries
                                                .get(&selected.identity.account_id),
                                            &selected.identity,
                                        ))
                                        .unwrap();
                                    }
                                    if let Some(started) = started {
                                        latencies.push(started.elapsed().as_nanos());
                                    }
                                }
                                observed.send((false, latencies)).unwrap();
                            }));
                        }
                        if writes != 0 {
                            let barrier = barrier.clone();
                            let baseline = baseline.clone();
                            let observed = observed.clone();
                            let mut unrelated = descriptor;
                            unrelated.id = "unrelated".into();
                            tasks.push(std::thread::spawn(move || {
                                let mut latencies = Vec::new();
                                barrier.wait();
                                for _ in 0..writes {
                                    let started = Instant::now();
                                    if published {
                                        pool.register(unrelated.clone(), Arc::new(Unused)).unwrap();
                                    } else {
                                        let descriptor = unrelated.clone();
                                        descriptor.validate().unwrap();
                                        let provider: Arc<dyn Provider> = Arc::new(Unused);
                                        assert!(
                                            descriptor.inference_executor == "local"
                                                && provider.delegated_binding().is_none()
                                        );
                                        let mut state = baseline.write().unwrap();
                                        assert!(
                                            state.entries.len() < 128
                                                || state.entries.contains_key(&descriptor.id)
                                        );
                                        state.revision = state.revision.checked_add(1).unwrap();
                                        let revision = state.revision;
                                        state.entries.insert(
                                            descriptor.id.clone(),
                                            Entry {
                                                descriptor,
                                                provider,
                                                revision,
                                            },
                                        );
                                    }
                                    latencies.push(started.elapsed().as_nanos());
                                    std::thread::yield_now();
                                }
                                observed.send((true, latencies)).unwrap();
                            }));
                        }
                        let started = Instant::now();
                        barrier.wait();
                        for task in tasks {
                            task.join().unwrap();
                        }
                        let ns_per_read = started.elapsed().as_nanos() / (threads * READS) as u128;
                        drop(observed);
                        let mut read_samples = Vec::new();
                        let mut write_samples = Vec::new();
                        for (write, values) in samples {
                            if write {
                                write_samples.extend(values);
                            } else {
                                read_samples.extend(values);
                            }
                        }
                        timings.push((
                            ns_per_read,
                            percentiles(read_samples),
                            percentiles(write_samples),
                        ));
                    }
                    eprintln!(
                        "{}",
                        serde_json::json!({"catalog_size":catalog_size,"threads":threads,
                        "reads_each":READS,"writes":writes,"updates_per_thousand_reads":per_thousand,
                        "sample_stride":SAMPLE_STRIDE,"rwlock":timings[0],"publication":timings[1],
                        "units":"ns_per_read_including_writes, read_p50_p95_p99_ns, write_p50_p95_p99_ns"})
                    );
                }
            }
        }
        let (_, _, _, selected) = fixture();
        let started = Instant::now();
        for _ in 0..READS {
            std::hint::black_box(selected.revalidate()).unwrap();
        }
        eprintln!(
            "accounts cold_reader_setup_ns={}",
            started.elapsed().as_nanos() / READS as u128
        );
        let (pool, descriptor, _, _) = fixture();
        let baseline = RwLock::new(legacy_state(&pool));
        let started = Instant::now();
        for _ in 0..10_000 {
            let descriptor = descriptor.clone();
            descriptor.validate().unwrap();
            let provider: Arc<dyn Provider> = Arc::new(Unused);
            let mut state = baseline.write().unwrap();
            state.revision += 1;
            let revision = state.revision;
            state.entries.insert(
                descriptor.id.clone(),
                Entry {
                    descriptor,
                    provider,
                    revision,
                },
            );
        }
        let old_ns = started.elapsed().as_nanos() / 10_000;
        let started = Instant::now();
        for _ in 0..10_000 {
            pool.register(descriptor.clone(), Arc::new(Unused)).unwrap();
        }
        eprintln!(
            "accounts isolated_register_rwlock_ns={old_ns} isolated_register_publication_ns={}",
            started.elapsed().as_nanos() / 10_000
        );
    }
}

#[cfg(test)]
mod delegated_tests {
    use super::*;
    struct Remote;
    impl Provider for Remote {
        fn delegated_binding(&self) -> Option<crate::DelegatedBinding> {
            Some(crate::DelegatedBinding {
                account_id: "personal".into(),
                executor_id: "laptop".into(),
            })
        }
        fn stream<'a>(
            &'a self,
            _: &'a [crate::Message],
            _: &'a str,
            _: &'a mut crate::DeltaSink<'_>,
        ) -> crate::CompletionFuture<'a> {
            Box::pin(async { Err(Error::Denied("fixture is metadata-only")) })
        }
    }
    #[test]
    fn normal_register_cannot_smuggle_remote_provider() {
        let p = ProviderAccounts::new("server").unwrap();
        let u = PrincipalId::new("alice").unwrap();
        assert!(p
            .register(
                AccountDescriptor::personal("personal", u, "laptop", ["m".into()]),
                Arc::new(Remote)
            )
            .is_err());
    }
    #[test]
    fn delegated_registration_preserves_personal_payer() {
        let p = ProviderAccounts::new("server").unwrap();
        let u = PrincipalId::new("alice").unwrap();
        p.register_delegated(
            AccountDescriptor::personal("personal", u.clone(), "laptop", ["m".into()]),
            Arc::new(Remote),
        )
        .unwrap();
        let a = p.select(&u, "personal", "m").unwrap();
        assert_eq!(a.identity().payer, BillingOwner::User(u));
    }
    #[test]
    fn delegated_registration_denies_other_user() {
        let p = ProviderAccounts::new("server").unwrap();
        let u = PrincipalId::new("alice").unwrap();
        p.register_delegated(
            AccountDescriptor::personal("personal", u, "laptop", ["m".into()]),
            Arc::new(Remote),
        )
        .unwrap();
        assert!(p
            .select(&PrincipalId::new("bob").unwrap(), "personal", "m")
            .is_err());
    }
    #[test]
    fn delegated_registration_cannot_replace_account() {
        let p = ProviderAccounts::new("server").unwrap();
        let u = PrincipalId::new("alice").unwrap();
        let d = AccountDescriptor::personal("personal", u, "laptop", ["m".into()]);
        p.register_delegated(d.clone(), Arc::new(Remote)).unwrap();
        assert!(p.register_delegated(d, Arc::new(Remote)).is_err());
    }
}
