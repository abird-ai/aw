//! AW-P6-8B: strictly host-owner-bound encrypted extension secret API.
//! This adapts the original current Actor/Session Runtime binding to the
//! existing v33 SQLite audit + original ChaCha20-Poly1305 LocalSecretStore.
//! No generic web/stdio/MCP API, model prompt/Hook, or encrypted ref export.
use crate::{actor_owner::Service, Error, Result};
use aw_store::{SecretSettingScope, SecretSettingView, SessionStore};
use aw_ui_extension::settings::{SettingKind, SettingsRegistry};
use zeroize::Zeroizing;

fn scope(owner: &Service, extension: &str, name: &str) -> Result<SecretSettingScope> {
    owner.verify_rpc_state()?;
    let (_, session, branch, site, writer_epoch) = owner.bound_peer_scope();
    Ok(SecretSettingScope {
        session_id: session.to_owned(),
        branch_id: branch.to_owned(),
        site: site.to_owned(),
        extension: extension.to_owned(),
        name: name.to_owned(),
        writer_epoch,
    })
}
fn maximum(registry: &SettingsRegistry, extension: &str, name: &str) -> Result<u16> {
    let setting = registry.definition(extension, name).map_err(|_| {
        Error::Denied("extension secret is not registered by a trusted current manifest")
    })?;
    match setting.kind {
        SettingKind::Secret { max_bytes } => Ok(max_bytes),
        _ => Err(Error::Denied(
            "setting is not a separately authorized vault secret",
        )),
    }
}
/// Explicit host-only SET; expected_revision=None when unset. A user-visible
/// `SecretSettingView` is redacted; do not log the caller's secret bytes.
pub fn set(
    store: &SessionStore,
    owner: &Service,
    registry: &SettingsRegistry,
    extension: &str,
    name: &str,
    expected_revision: Option<u64>,
    secret: &[u8],
) -> Result<SecretSettingView> {
    let max = maximum(registry, extension, name)?;
    let bound = scope(owner, extension, name)?;
    let (principal, ..) = owner.bound_peer_scope();
    store.extension_secret_set(principal, bound, expected_revision, max, secret)
}
/// Explicit host-only owner READ into zeroizing memory. No printable
/// plaintext result, no JSON/CLI transport and no `Debug` on the returned body.
pub fn read(
    store: &SessionStore,
    owner: &Service,
    registry: &SettingsRegistry,
    extension: &str,
    name: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    maximum(registry, extension, name)?;
    let bound = scope(owner, extension, name)?;
    let (principal, ..) = owner.bound_peer_scope();
    store.extension_secret_read(principal, &bound)
}
pub fn reset(
    store: &SessionStore,
    owner: &Service,
    registry: &SettingsRegistry,
    extension: &str,
    name: &str,
    expected_revision: u64,
) -> Result<SecretSettingView> {
    maximum(registry, extension, name)?;
    let bound = scope(owner, extension, name)?;
    let (principal, ..) = owner.bound_peer_scope();
    store.extension_secret_reset(principal, bound, expected_revision)
}
pub fn view(
    store: &SessionStore,
    owner: &Service,
    registry: &SettingsRegistry,
    extension: &str,
    name: &str,
) -> Result<Option<SecretSettingView>> {
    maximum(registry, extension, name)?;
    let bound = scope(owner, extension, name)?;
    let (principal, ..) = owner.bound_peer_scope();
    store.extension_secret_view(principal, &bound)
}
/// Filter to the exact current host-registered Secret kinds. A stale unknown
/// registry key must not be advertised as configured or implicitly activated.
pub fn list(
    store: &SessionStore,
    owner: &Service,
    registry: &SettingsRegistry,
) -> Result<Vec<SecretSettingView>> {
    let bound = scope(owner, "secret_scope", "list")?;
    let (principal, ..) = owner.bound_peer_scope();
    Ok(store
        .extension_secret_list(principal, &bound)?
        .into_iter()
        .filter(|view| maximum(registry, &view.extension, &view.name).is_ok())
        .collect())
}
pub fn reclaim_retired(store: &SessionStore, owner: &Service, limit: usize) -> Result<usize> {
    let bound = scope(owner, "secret_scope", "reclaim")?;
    let (principal, ..) = owner.bound_peer_scope();
    store.extension_secret_reclaim_retired(principal, &bound, limit)
}
