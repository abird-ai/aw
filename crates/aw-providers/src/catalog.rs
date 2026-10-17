//! Read-only model metadata, private per-profile discovery cache, and explicit
//! user-priced USD token estimates. Descriptive metadata never grants a model,
//! account, tool, provider fallback, or permission to run network requests.
//!
//! Only an explicitly requested refresh or guided setup contacts a provider.
use crate::{
    config::{validate_model, Profile, ProviderKind},
    Error, Result,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const VERSION: u8 = 1;
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_PRICE_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin {
    Configured,
    BundledReference,
    ProviderDiscovery,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceOrigin {
    ProviderReported,
    UserOverride,
}

/// All rate values are **micro-USD per one million tokens**, not floating-point
/// prices and not invoices. Nonexistent rates are truly unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    pub input_micro_usd_per_mtok: u64,
    pub output_micro_usd_per_mtok: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_micro_usd_per_mtok: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_micro_usd_per_mtok: Option<u64>,
    pub origin: PriceOrigin,
}
impl Pricing {
    pub fn validate(&self) -> Result<()> {
        // $0 is valid; a huge number in a bogus endpoint response is not.
        for n in [
            Some(self.input_micro_usd_per_mtok),
            Some(self.output_micro_usd_per_mtok),
            self.cache_read_micro_usd_per_mtok,
            self.cache_write_micro_usd_per_mtok,
        ]
        .into_iter()
        .flatten()
        {
            if n > 100_000_000_000 {
                return Err(Error::Limit("model price exceeds $100k per million tokens"));
            }
        }
        Ok(())
    }
    /// Exact result is integer micro-USD; ambiguous cached-input accounting
    /// remains unknown rather than double-counting it as new prompt tokens.
    pub fn estimate_micro_usd(&self, ordinary_input: u64, output: u64) -> Option<u64> {
        self.validate().ok()?;
        let total = u128::from(self.input_micro_usd_per_mtok)
            .checked_mul(u128::from(ordinary_input))?
            .checked_add(
                u128::from(self.output_micro_usd_per_mtok).checked_mul(u128::from(output))?,
            )?;
        u64::try_from(total.checked_add(500_000)? / 1_000_000).ok()
    }
    pub fn estimate_with_cache_micro_usd(
        &self,
        ordinary_input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> Option<u64> {
        self.validate().ok()?;
        let mut numerator = u128::from(self.input_micro_usd_per_mtok)
            .checked_mul(u128::from(ordinary_input))?
            .checked_add(
                u128::from(self.output_micro_usd_per_mtok).checked_mul(u128::from(output))?,
            )?;
        if cache_read > 0 {
            numerator = numerator.checked_add(
                u128::from(self.cache_read_micro_usd_per_mtok?)
                    .checked_mul(u128::from(cache_read))?,
            )?;
        }
        if cache_write > 0 {
            numerator = numerator.checked_add(
                u128::from(self.cache_write_micro_usd_per_mtok?)
                    .checked_mul(u128::from(cache_write))?,
            )?;
        }
        u64::try_from(numerator.checked_add(500_000)? / 1_000_000).ok()
    }
}

/// Decimal USD per million tokens -> integer microUSD per million tokens.
/// Neither floats nor scientific notation are accepted; excess precision
/// is refused rather than silently rounded or truncated.
pub fn parse_usd_per_mtok(input: &str) -> Result<u64> {
    parse_scaled_decimal(input, 6)
}
/// USD per token (as returned by some provider catalogs), converted to
/// microUSD per million tokens exactly (scale 10^12).
pub(crate) fn parse_usd_per_token(input: &str) -> Result<u64> {
    parse_scaled_decimal(input, 12)
}
fn parse_scaled_decimal(input: &str, decimals: usize) -> Result<u64> {
    let (whole, fractional) = input.split_once('.').unwrap_or((input, ""));
    if whole.is_empty()
        || (input.contains('.') && fractional.is_empty())
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fractional.bytes().all(|b| b.is_ascii_digit())
        || fractional.len() > decimals
        || input.len() > 32
    {
        return Err(Error::Config(
            "price must be a nonnegative plain decimal with bounded precision".into(),
        ));
    }
    let scale = 10_u64.pow(decimals as u32);
    let w = whole
        .parse::<u64>()
        .map_err(|_| Error::Limit("price exceeds numeric bounds"))?;
    let fractional = if fractional.is_empty() {
        0
    } else {
        fractional
            .parse::<u64>()
            .map_err(|_| Error::Limit("price fraction exceeds numeric bounds"))?
    };
    let amount = w
        .checked_mul(scale)
        .and_then(|n| {
            n.checked_add(
                fractional.checked_mul(10_u64.pow((decimals - fractional_digits(input)) as u32))?,
            )
        })
        .ok_or(Error::Limit("price overflows integer rate precision"))?;
    if amount > 100_000_000_000 {
        return Err(Error::Limit("model price exceeds $100k per million tokens"));
    }
    Ok(amount)
}
fn fractional_digits(input: &str) -> usize {
    input.split_once('.').map_or(0, |(_, s)| s.len())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<Pricing>,
    pub origin: ModelOrigin,
}
impl ModelInfo {
    pub fn configured(id: String) -> Self {
        Self {
            id,
            display_name: None,
            reference_as_of: None,
            input_tokens: None,
            output_tokens: None,
            context_tokens: None,
            supports_tools: None,
            supports_thinking: None,
            lifecycle: None,
            pricing: None,
            origin: ModelOrigin::Configured,
        }
    }
    pub fn validate(&self) -> Result<()> {
        validate_model(&self.id)?;
        if self.reference_as_of.as_deref().is_some_and(|d| {
            d.len() != 10
                || !d.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
        }) {
            return Err(Error::Config("invalid model reference-as-of date".into()));
        }
        if self
            .display_name
            .as_deref()
            .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
        {
            return Err(Error::Limit("model display name invalid"));
        }
        for limit in [self.input_tokens, self.output_tokens, self.context_tokens]
            .into_iter()
            .flatten()
        {
            if limit == 0 || limit > 1_000_000_000 {
                return Err(Error::Limit("model token limit outside supported range"));
            }
        }
        if self
            .lifecycle
            .as_deref()
            .is_some_and(|v| !matches!(v, "active" | "deprecated" | "retired"))
        {
            return Err(Error::Config("model lifecycle unsupported".into()));
        }
        if let Some(p) = &self.pricing {
            p.validate()?
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSnapshot {
    pub version: u8,
    pub kind: ProviderKind,
    pub base_url: String,
    /// Salted key verifier. Never contains an API key or a raw unkeyed token digest.
    pub credential_tag: String,
    pub salt_hex: String,
    pub fetched_at: u64,
    pub models: Vec<ModelInfo>,
}
impl CatalogSnapshot {
    pub fn fresh(&self, now: u64) -> bool {
        self.fetched_at <= now && now - self.fetched_at <= CACHE_TTL_SECS
    }
    fn validate(&self) -> Result<()> {
        if self.version != VERSION
            || self.models.is_empty()
            || self.models.len() > MAX_ENTRIES
            || self.salt_hex.len() != 64
            || self.credential_tag.len() != 64
            || !self.salt_hex.bytes().all(|b| b.is_ascii_hexdigit())
            || !self.credential_tag.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Config(
                "invalid or incompatible saved model catalog".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for m in &self.models {
            m.validate()?;
            if !ids.insert(&m.id) {
                return Err(Error::Config("duplicate model IDs in saved catalog".into()));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PriceOverrides {
    version: u8,
    kind: ProviderKind,
    base_url: String,
    /// A private salted verifier for the exact selected account credential.
    credential_tag: String,
    salt_hex: String,
    models: BTreeMap<String, Pricing>,
}
impl PriceOverrides {
    fn fresh(kind: ProviderKind, base_url: String) -> Self {
        Self {
            version: VERSION,
            kind,
            base_url,
            credential_tag: String::new(),
            salt_hex: String::new(),
            models: BTreeMap::new(),
        }
    }
    fn validate(&self) -> Result<()> {
        if self.version != VERSION
            || self.models.len() > MAX_ENTRIES
            || (self.salt_hex.len() != 64 || self.credential_tag.len() != 64)
                && (!self.models.is_empty()
                    || !self.salt_hex.is_empty()
                    || !self.credential_tag.is_empty())
        {
            return Err(Error::Config("invalid price override store".into()));
        }
        for (id, price) in &self.models {
            validate_model(id)?;
            price.validate()?;
            if price.origin != PriceOrigin::UserOverride {
                return Err(Error::Denied(
                    "price override must be labelled user_override",
                ));
            }
        }
        Ok(())
    }
}

fn now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Config("clock predates Unix epoch".into()))?
        .as_secs())
}
pub(crate) fn credential(profile: &Profile) -> Result<Zeroizing<Vec<u8>>> {
    if profile.oauth_connector.is_some() {
        return Err(Error::Denied(
            "generic model discovery cannot reuse a subscription/OAuth credential",
        ));
    }
    if let Some(env) = &profile.api_key_env {
        let text = std::env::var(env).map_err(|_| {
            Error::Config(format!(
                "selected API credential {env} is unavailable for catalog cache"
            ))
        })?;
        if text.is_empty() || text.len() > 65_536 || text.contains(['\r', '\n']) {
            return Err(Error::Config("selected API credential invalid".into()));
        }
        Ok(Zeroizing::new(text.into_bytes()))
    } else {
        Ok(Zeroizing::new(Vec::new()))
    }
}
fn salted_tag(salt: &[u8], secret: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"aw-model-catalog-secret-v1");
    h.update(salt);
    h.update(secret);
    format!("{:x}", h.finalize())
}
fn decode_salt(value: &str) -> Result<[u8; 32]> {
    let bytes = value.as_bytes();
    if bytes.len() != 64 {
        return Err(Error::Config("invalid catalog salt".into()));
    }
    let mut salt = [0; 32];
    for (i, destination) in salt.iter_mut().enumerate() {
        let hi = (bytes[2 * i] as char)
            .to_digit(16)
            .ok_or(Error::Config("invalid salt encoding".into()))?;
        let lo = (bytes[2 * i + 1] as char)
            .to_digit(16)
            .ok_or(Error::Config("invalid salt encoding".into()))?;
        *destination = ((hi << 4) | lo) as u8;
    }
    Ok(salt)
}
fn fresh_salt() -> Result<[u8; 32]> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::Denied("no secure entropy to scope model-cache credential"))?;
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
pub fn default_cache_dir() -> Result<PathBuf> {
    let base = if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        let p = PathBuf::from(cache);
        if !p.is_absolute() {
            return Err(Error::Config(
                "XDG_CACHE_HOME must be an absolute path".into(),
            ));
        }
        p
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or(Error::Config(
            "set HOME or XDG_CACHE_HOME for model discovery cache".into(),
        ))?)
        .join(".cache")
    };
    Ok(base.join("abird/aw/model-catalog"))
}
fn profile_key(profile: &Profile) -> String {
    let mut h = Sha256::new();
    h.update(b"aw-model-provider-endpoint-v1");
    h.update(profile.kind.label().as_bytes());
    h.update([0]);
    h.update(profile.base_url.as_bytes());
    h.update([0]);
    h.update(profile.api_key_env.as_deref().unwrap_or("").as_bytes());
    h.update([0]);
    h.update(profile.oauth_connector.as_deref().unwrap_or("").as_bytes());
    hex(&h.finalize())
}

/// Lazy, user-local metadata cache. Does not create directories or call a
/// provider merely by being constructed or read on a cache miss.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}
impl Cache {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn default_user() -> Result<Self> {
        Ok(Self::at(default_cache_dir()?))
    }
    fn path(&self, profile: &Profile, suffix: &str) -> PathBuf {
        self.root
            .join(format!("{}.{}", profile_key(profile), suffix))
    }
    fn read_bytes(&self, path: &Path, max: usize) -> Result<Option<Vec<u8>>> {
        match std::fs::symlink_metadata(&self.root) {
            Ok(_) => aw_platform::ensure_private_dir(&self.root)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let file = match aw_platform::open_private_existing(path, false) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut body = Vec::new();
        file.take((max + 1) as u64).read_to_end(&mut body)?;
        if body.len() > max {
            return Err(Error::Limit("model metadata file exceeds size limit"));
        }
        Ok(Some(body))
    }
    pub fn load(&self, profile: &Profile) -> Result<Option<CatalogSnapshot>> {
        profile.validate()?;
        let path = self.path(profile, "models.json");
        let Some(bytes) = self.read_bytes(&path, MAX_CATALOG_BYTES)? else {
            return Ok(None);
        };
        let data: CatalogSnapshot = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Config("invalid model catalog cache JSON".into()))?;
        data.validate()?;
        let secret = match credential(profile) {
            Ok(secret) => secret,
            // Offline listings may still show configured models when the
            // selected account key is unavailable, but NEVER cached names
            // admitted under a different key.
            Err(Error::Config(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let salt = decode_salt(&data.salt_hex)?;
        if data.kind != profile.kind
            || data.base_url != profile.base_url
            || data.credential_tag != salted_tag(&salt, &secret)
        {
            return Ok(None);
        }
        Ok(Some(data))
    }
    fn write(&self, path: &Path, body: &[u8]) -> Result<()> {
        aw_platform::ensure_private_dir(&self.root)?;
        aw_platform::atomic_write_user_metadata(path, body)?;
        Ok(())
    }
    pub fn store(
        &self,
        profile: &Profile,
        models: Vec<ModelInfo>,
        at: u64,
    ) -> Result<CatalogSnapshot> {
        profile.validate()?;
        let secret = credential(profile)?;
        self.store_with_selected_key(profile, models, at, &secret)
    }
    fn store_with_selected_key(
        &self,
        profile: &Profile,
        models: Vec<ModelInfo>,
        at: u64,
        secret: &[u8],
    ) -> Result<CatalogSnapshot> {
        let salt = fresh_salt()?;
        let snapshot = CatalogSnapshot {
            version: VERSION,
            kind: profile.kind,
            base_url: profile.base_url.clone(),
            credential_tag: salted_tag(&salt, secret),
            salt_hex: hex(&salt),
            fetched_at: at,
            models,
        };
        snapshot.validate()?;
        let body = serde_json::to_vec(&snapshot)
            .map_err(|_| Error::Config("cannot serialize model catalog".into()))?;
        if body.len() > MAX_CATALOG_BYTES {
            return Err(Error::Limit("model catalog exceeds cache capacity"));
        }
        self.write(&self.path(profile, "models.json"), &body)?;
        Ok(snapshot)
    }
    fn load_prices(&self, profile: &Profile) -> Result<PriceOverrides> {
        profile.validate()?;
        let path = self.path(profile, "prices.json");
        let Some(body) = self.read_bytes(&path, MAX_PRICE_BYTES)? else {
            return Ok(PriceOverrides::fresh(
                profile.kind,
                profile.base_url.clone(),
            ));
        };
        let data: PriceOverrides = serde_json::from_slice(&body)
            .map_err(|_| Error::Config("invalid model price overrides JSON".into()))?;
        data.validate()?;
        if data.kind != profile.kind || data.base_url != profile.base_url {
            return Err(Error::Denied(
                "price overrides bound to another provider destination",
            ));
        }
        let selected = match credential(profile) {
            Ok(value) => value,
            Err(Error::Config(_)) => {
                return Ok(PriceOverrides::fresh(
                    profile.kind,
                    profile.base_url.clone(),
                ))
            }
            Err(error) => return Err(error),
        };
        if data.salt_hex.is_empty() && data.credential_tag.is_empty() {
            return Ok(data);
        }
        if data.credential_tag != salted_tag(&decode_salt(&data.salt_hex)?, &selected) {
            return Ok(PriceOverrides::fresh(
                profile.kind,
                profile.base_url.clone(),
            ));
        }
        Ok(data)
    }
    pub fn price(&self, profile: &Profile, model: &str) -> Result<Option<Pricing>> {
        validate_model(model)?;
        Ok(self.load_prices(profile)?.models.get(model).cloned())
    }
    pub fn set_price(&self, profile: &Profile, model: &str, mut price: Pricing) -> Result<()> {
        validate_model(model)?;
        price.origin = PriceOrigin::UserOverride;
        price.validate()?;
        self.mutate_prices(profile, |data| {
            if !data.models.contains_key(model) && data.models.len() >= MAX_ENTRIES {
                return Err(Error::Limit("too many model price overrides"));
            }
            data.models.insert(model.into(), price);
            Ok(())
        })
    }
    pub fn remove_price(&self, profile: &Profile, model: &str) -> Result<()> {
        validate_model(model)?;
        self.mutate_prices(profile, |data| {
            data.models.remove(model);
            Ok(())
        })
    }
    fn mutate_prices(
        &self,
        profile: &Profile,
        change: impl FnOnce(&mut PriceOverrides) -> Result<()>,
    ) -> Result<()> {
        let path = self.path(profile, "prices.json");
        aw_platform::ensure_private_dir(&self.root)?;
        let lock = aw_platform::open_user_metadata_lock(&path.with_extension("prices.lock"))?;
        lock.lock_exclusive()?;
        let secret = credential(profile)?;
        let mut prices = self.load_prices(profile)?;
        change(&mut prices)?;
        if prices.credential_tag.is_empty() {
            let salt = fresh_salt()?;
            prices.salt_hex = hex(&salt);
            prices.credential_tag = salted_tag(&salt, &secret);
        }
        prices.validate()?;
        let data = serde_json::to_vec(&prices)
            .map_err(|_| Error::Config("invalid model price overrides".into()))?;
        if data.len() > MAX_PRICE_BYTES {
            return Err(Error::Limit("model price overrides too large"));
        }
        self.write(&path, &data)?;
        drop(lock);
        Ok(())
    }
}

/// Small dated reference snapshots for first-use/offline discovery. This is
/// *never* account availability or an inference allowlist, and has no price.
/// Provider discovery supersedes it when the user explicitly connects.
fn bundled_reference(profile: &Profile) -> Vec<ModelInfo> {
    type ReferenceRow = (&'static str, Option<u64>, Option<u64>, Option<bool>);
    let models: &[ReferenceRow] = match (profile.kind, profile.base_url.trim_end_matches('/')) {
        (ProviderKind::Openai | ProviderKind::OpenaiResponses, "https://api.openai.com/v1") => &[
            ("gpt-6-astra", None, Some(128_000), Some(true)),
            ("gpt-6.1-sol", None, Some(128_000), Some(true)),
            ("gpt-6-luna", None, Some(128_000), Some(true)),
        ],
        (ProviderKind::Anthropic, "https://api.anthropic.com/v1") => &[
            (
                "claude-fable-5-1",
                Some(1_000_000),
                Some(128_000),
                Some(true),
            ),
            (
                "claude-opus-5-5",
                Some(1_000_000),
                Some(128_000),
                Some(true),
            ),
            (
                "claude-sonnet-5-5",
                Some(1_000_000),
                Some(128_000),
                Some(true),
            ),
            (
                "claude-haiku-5-5",
                Some(1_000_000),
                Some(128_000),
                Some(true),
            ),
        ],
        (ProviderKind::Gemini, "https://generativelanguage.googleapis.com/v1beta") => &[
            ("gemini-3.8-flash", None, None, Some(true)),
            ("gemini-3.7-flash", None, None, Some(true)),
        ],
        _ => &[],
    };
    models
        .iter()
        .map(|(id, context, output, thinking)| {
            let mut info = ModelInfo::configured((*id).into());
            info.origin = ModelOrigin::BundledReference;
            info.reference_as_of = Some("2026-10-09".into());
            info.context_tokens = *context;
            info.input_tokens = *context;
            info.output_tokens = *output;
            info.supports_thinking = *thinking;
            info
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct View {
    pub provider: String,
    pub endpoint: String,
    pub fetched_at: Option<u64>,
    pub fresh: bool,
    pub models: Vec<ModelInfo>,
}
impl View {
    pub fn merge(
        profile: &Profile,
        snapshot: Option<&CatalogSnapshot>,
        prices: &BTreeMap<String, Pricing>,
        now: u64,
    ) -> Result<Self> {
        let mut models = BTreeMap::<String, ModelInfo>::new();
        for info in bundled_reference(profile) {
            models.insert(info.id.clone(), info);
        }
        for id in profile.effective_models() {
            models
                .entry(id.clone())
                .or_insert_with(|| ModelInfo::configured(id));
        }
        if let Some(s) = snapshot {
            for item in &s.models {
                item.validate()?;
                models.insert(item.id.clone(), item.clone());
            }
        }
        // A local override is descriptive only; never makes a model available
        // to an account, and never grants permission to run a discovered ID.
        for (id, price) in prices {
            if let Some(item) = models.get_mut(id) {
                item.pricing = Some(price.clone());
            }
        }
        Ok(Self {
            provider: profile.kind.label().into(),
            endpoint: profile.base_url.clone(),
            fetched_at: snapshot.map(|s| s.fetched_at),
            fresh: snapshot.is_some_and(|s| s.fresh(now)),
            models: models.into_values().collect(),
        })
    }
}
impl Cache {
    pub fn view(&self, profile: &Profile) -> Result<View> {
        let snapshot = self.load(profile)?;
        let prices = self.load_prices(profile)?.models;
        View::merge(profile, snapshot.as_ref(), &prices, now()?)
    }
    pub async fn refresh(&self, profile: &Profile) -> Result<View> {
        let secret = credential(profile)?;
        let models = crate::discovery::fetch_catalog(profile, &secret).await?;
        let saved = self.store_with_selected_key(profile, models, now()?, &secret)?;
        View::merge(
            profile,
            Some(&saved),
            &self.load_prices(profile)?.models,
            now()?,
        )
    }
    pub async fn for_setup(&self, profile: &Profile) -> Result<Vec<String>> {
        // A first-run model picker must suggest provider-reported candidates,
        // not bundled reference IDs the selected account may lack.
        let models = match self.load(profile)? {
            Some(snapshot) if snapshot.fresh(now()?) => snapshot.models,
            _ => self
                .refresh(profile)
                .await?
                .models
                .into_iter()
                .filter(|item| item.origin == ModelOrigin::ProviderDiscovery)
                .collect(),
        };
        Ok(models.into_iter().map(|item| item.id).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "aw-model-catalog-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }
    #[test]
    fn private_salted_account_cache_fences_key_changes_and_offline_metadata() {
        let root = temp_dir("secret");
        const ENV: &str = "AW_R4_CATALOG_TEST_TOKEN_SECRET";
        let cache = Cache::at(&root);
        let mut p = crate::config::builtins().remove("gemini").unwrap();
        p.base_url = "https://account.invalid/v1beta".into();
        p.api_key_env = Some(ENV.into());
        p.model = Some("gemini-test".into());
        std::env::remove_var(ENV);
        let offline = cache.view(&p).unwrap();
        assert_eq!(offline.models.len(), 1);
        assert_eq!(offline.models[0].id, "gemini-test");
        assert!(
            !root.exists(),
            "listing on a cache miss must not create files"
        );
        std::env::set_var(ENV, "PRIVATE-FIXTURE-KEY-1");
        let mut item = ModelInfo::configured("gemini-test".into());
        item.origin = ModelOrigin::ProviderDiscovery;
        item.input_tokens = Some(1_000_000);
        item.output_tokens = Some(65_536);
        let at = now().unwrap();
        let saved = cache.store(&p, vec![item.clone()], at).unwrap();
        assert!(saved.fresh(at));
        assert!(!saved.fresh(at + CACHE_TTL_SECS + 1));
        let stored = std::fs::read_to_string(cache.path(&p, "models.json")).unwrap();
        for forbidden in ["PRIVATE-FIXTURE-KEY-1", "GEMINI_API_KEY"] {
            assert!(
                !stored.contains(forbidden),
                "{forbidden} leaked into saved cache"
            );
        }
        assert_eq!(
            cache.view(&p).unwrap().models[0].input_tokens,
            Some(1_000_000)
        );
        std::env::set_var(ENV, "PRIVATE-FIXTURE-KEY-2");
        assert!(
            cache.load(&p).unwrap().is_none(),
            "another payer must not inherit cached account models"
        );
        let changed = cache.view(&p).unwrap();
        assert_eq!(changed.models.len(), 1);
        assert_eq!(changed.models[0].input_tokens, None);
        std::env::remove_var(ENV);
        assert!(cache.load(&p).unwrap().is_none());
        std::env::set_var(ENV, "PRIVATE-FIXTURE-KEY-1");
        assert!(cache.load(&p).unwrap().is_some());
        let mut duplicate = item.clone();
        duplicate.display_name = Some("another".into());
        assert!(cache.store(&p, vec![item, duplicate], at).is_err());
        let mut other = p.clone();
        other.base_url = "https://other.invalid/v1beta".into();
        assert!(cache.load(&other).unwrap().is_none());
        std::env::remove_var(ENV);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn first_run_candidates_are_discovered_not_bundled_reference_models() {
        let root = temp_dir("setup");
        let mut profile = crate::config::builtins().remove("anthropic").unwrap();
        profile.api_key_env = None;
        let cache = Cache::at(&root);
        let bundled = cache.view(&profile).unwrap();
        assert!(bundled
            .models
            .iter()
            .any(|m| m.origin == ModelOrigin::BundledReference));
        let discovered = ModelInfo {
            origin: ModelOrigin::ProviderDiscovery,
            ..ModelInfo::configured("claude-account-specific".into())
        };
        cache
            .store(&profile, vec![discovered], now().unwrap())
            .unwrap();
        let first_run = cache.for_setup(&profile).await.unwrap();
        assert_eq!(first_run, vec!["claude-account-specific"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_prices_are_precise_private_locked_and_destination_scoped() {
        let root = temp_dir("prices");
        let cache = Cache::at(&root);
        let p = crate::config::builtins().remove("ollama").unwrap();
        let price = Pricing {
            input_micro_usd_per_mtok: parse_usd_per_mtok("3.25").unwrap(),
            output_micro_usd_per_mtok: parse_usd_per_mtok("18").unwrap(),
            cache_read_micro_usd_per_mtok: Some(parse_usd_per_mtok("0.4").unwrap()),
            cache_write_micro_usd_per_mtok: None,
            origin: PriceOrigin::UserOverride,
        };
        let c1 = cache.clone();
        let c2 = cache.clone();
        let p1 = p.clone();
        let p2 = p.clone();
        let price1 = price.clone();
        let price2 = price.clone();
        let a = std::thread::spawn(move || c1.set_price(&p1, "alpha", price1).unwrap());
        let b = std::thread::spawn(move || c2.set_price(&p2, "beta", price2).unwrap());
        a.join().unwrap();
        b.join().unwrap();
        assert_eq!(cache.price(&p, "alpha").unwrap(), Some(price.clone()));
        assert_eq!(cache.price(&p, "beta").unwrap(), Some(price.clone()));
        assert_eq!(price.estimate_micro_usd(1000, 1000), Some(21_250));
        assert_eq!(
            price.estimate_with_cache_micro_usd(1000, 1000, 1000, 0),
            Some(21_650)
        );
        assert_eq!(price.estimate_with_cache_micro_usd(1000, 1000, 0, 10), None);
        let mut other = p.clone();
        other.base_url = "http://127.0.0.1:19000".into();
        assert!(cache.price(&other, "alpha").unwrap().is_none());
        cache.remove_price(&p, "alpha").unwrap();
        assert!(cache.price(&p, "alpha").unwrap().is_none());
        assert!(cache.price(&p, "beta").unwrap().is_some());
        assert_eq!(
            cache.view(&p).unwrap().models.len(),
            0,
            "a price override is not model availability authority"
        );
        let listing = std::fs::read_dir(&root)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert!(!listing.is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in listing {
                assert_eq!(entry.metadata().unwrap().permissions().mode() & 0o077, 0);
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bundled_reference_is_descriptive_not_auto_selected_or_cross_host_available() {
        let mut p = crate::config::builtins().remove("anthropic").unwrap();
        let catalog = View::merge(&p, None, &BTreeMap::new(), 1).unwrap();
        assert!(catalog.models.iter().any(|m| m.id == "claude-sonnet-5-5"
            && m.context_tokens == Some(1_000_000)
            && m.origin == ModelOrigin::BundledReference
            && m.reference_as_of.as_deref() == Some("2026-10-09")
            && m.pricing.is_none()));
        assert_eq!(p.model, None, "reference catalog must not select a model");
        p.base_url = "https://custom-provider.example/v1".into();
        assert!(
            View::merge(&p, None, &BTreeMap::new(), 1)
                .unwrap()
                .models
                .is_empty(),
            "another host must never inherit official reference IDs"
        );
        let mut local = crate::config::builtins().remove("ollama").unwrap();
        local.model = Some("my-model".into());
        let catalog = View::merge(&local, None, &BTreeMap::new(), 1).unwrap();
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].origin, ModelOrigin::Configured);
        assert_eq!(catalog.models[0].pricing, None);
    }

    #[test]
    fn decimal_price_is_exact_and_scoped() {
        assert_eq!(parse_usd_per_mtok("3.25").unwrap(), 3_250_000);
        assert_eq!(parse_usd_per_token("0.00000325").unwrap(), 3_250_000);
        for bad in [
            "-1",
            "+1",
            "1e-6",
            "NaN",
            "",
            "0.0000001",
            "1.2345678",
            " 2",
            "3.",
        ] {
            assert!(parse_usd_per_mtok(bad).is_err(), "{bad}");
        }
        let price = Pricing {
            input_micro_usd_per_mtok: 3_000_000,
            output_micro_usd_per_mtok: 15_000_000,
            cache_read_micro_usd_per_mtok: None,
            cache_write_micro_usd_per_mtok: None,
            origin: PriceOrigin::UserOverride,
        };
        assert_eq!(price.estimate_micro_usd(1000, 1000), Some(18_000));
        assert_eq!(price.estimate_with_cache_micro_usd(1000, 1000, 1, 0), None);
        assert_eq!(
            price.estimate_with_cache_micro_usd(1000, 1000, 0, 0),
            Some(18_000)
        );
    }
}
