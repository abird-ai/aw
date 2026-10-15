//! AW-P6-8B: authenticated extension-secret custody in the EXISTING SQLite
//! v33 immutable Session audit + existing private write-once SecretStore.
//! No table, second vault, config plaintext, prompt injection, tool capability
//! or remembered executor. Every operation rechecks exact Session owner,
//! branch, site and active Workspace writer epoch. An audit fact is not a
//! successful physical erasure receipt or proof against arbitrary power loss.
use crate::{
    audit, branch_in, db, principal_is_session_owner, workspace_writer, SecretStore, SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use getrandom::fill;
use rusqlite::{params, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use zeroize::Zeroizing;

const ACTION: &str = "extension.secret";
const CUSTODY_ACTION: &str = "extension.secret.custody";
const MAX_AUDIT: usize = 4096;
const RESERVED_CLOSURES: usize = 64;
const MAX_ACTIVE: usize = 64;
const MAX_SECRET: usize = 2048;
const MAX_SWEEP: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSettingScope {
    pub session_id: String,
    pub branch_id: String,
    pub site: String,
    pub extension: String,
    pub name: String,
    pub writer_epoch: u64,
}
impl SecretSettingScope {
    fn validate(&self) -> Result<()> {
        if !label(&self.session_id, 128)
            || !label(&self.branch_id, 128)
            || !label(&self.site, 128)
            || !label(&self.extension, 64)
            || !label(&self.name, 64)
            || self.writer_epoch == 0
        {
            return Err(Error::Denied("invalid owner-bound extension secret scope"));
        }
        Ok(())
    }
}
fn label(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes().enumerate().all(|(index, b)| {
            b.is_ascii_alphanumeric()
                || (index > 0 && b"_.-".contains(&b))
                || (index > 0 && b == b':')
        })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSettingState {
    Active,
    Removed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretSettingFact {
    scope: SecretSettingScope,
    state: SecretSettingState,
    revision: u64,
    blob_ref: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSettingView {
    pub extension: String,
    pub name: String,
    pub state: SecretSettingState,
    pub revision: u64,
    pub configured: bool,
}
impl From<&SecretSettingFact> for SecretSettingView {
    fn from(f: &SecretSettingFact) -> Self {
        Self {
            extension: f.scope.extension.clone(),
            name: f.scope.name.clone(),
            state: f.state,
            revision: f.revision,
            configured: f.state == SecretSettingState::Active,
        }
    }
}
fn events(c: &rusqlite::Connection, session: &str) -> Result<Vec<SecretSettingFact>> {
    let mut statement = db(c.prepare(
        "SELECT data_json FROM audit WHERE session_id=?1 AND action=?2 ORDER BY seq LIMIT 4097",
    ))?;
    let records = db(statement.query_map(params![session, ACTION], |r| r.get::<_, String>(0)))?;
    let rows = db(records.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_AUDIT {
        return Err(Error::Limit("extension secret audit needs owner review"));
    }
    rows.iter().map(|encoded| crate::decode(encoded)).collect()
}
fn key(scope: &SecretSettingScope) -> (String, String, String, String) {
    (
        scope.branch_id.clone(),
        scope.site.clone(),
        scope.extension.clone(),
        scope.name.clone(),
    )
}
fn latest(
    facts: &[SecretSettingFact],
) -> BTreeMap<(String, String, String, String), SecretSettingFact> {
    let mut last = BTreeMap::new();
    for fact in facts {
        last.insert(key(&fact.scope), fact.clone());
    }
    last
}
/// AW-P6-10O: bounded, strictly read-only projection of the LATEST original
/// immutable Extension Secret state per exact Session/branch/site/name. This
/// returns *internal* refs to the original operator-only custody validator,
/// never to an Agent, Web/RPC caller, or model-visible context. The caller
/// enforces a global immutable-audit size bound before passing rows here.
pub(crate) fn active_refs_for_custody_inventory(rows: &[(String, String)]) -> Result<Vec<String>> {
    let mut latest = BTreeMap::new();
    for (session, json) in rows {
        let fact: SecretSettingFact = crate::decode(json)?;
        if fact.scope.session_id != *session {
            return Err(Error::Denied(
                "Extension Secret audit Session identity changed",
            ));
        }
        latest.insert((session.clone(), key(&fact.scope)), fact);
    }
    let mut active = Vec::new();
    for fact in latest.into_values() {
        if fact.state == SecretSettingState::Active {
            active.push(fact.blob_ref.ok_or(Error::Denied(
                "active Extension Secret lacks an original encrypted custody reference",
            ))?);
        }
    }
    Ok(active)
}

fn scope_current(
    c: &rusqlite::Connection,
    owner: &PrincipalId,
    scope: &SecretSettingScope,
) -> Result<()> {
    if !principal_is_session_owner(c, &scope.session_id, owner)? {
        return Err(Error::Denied(
            "extension vault requires exact existing Session owner",
        ));
    }
    let writer = workspace_writer::verify_writer_epoch_for_session(
        c,
        &scope.session_id,
        scope.writer_epoch,
    )?;
    if writer.state != "active" {
        return Err(Error::Denied("extension vault writer no longer active"));
    }
    let branch = branch_in(c, &scope.session_id, &scope.branch_id)?;
    if branch.id != scope.branch_id {
        return Err(Error::Denied(
            "extension secret needs canonical Session branch ID",
        ));
    }
    Ok(())
}
fn random_ref(scope: &SecretSettingScope) -> Result<String> {
    let mut nonce = [0u8; 32];
    fill(&mut nonce)
        .map_err(|_| Error::Storage("OS randomness unavailable for extension-secret custody"))?;
    let mut hasher = Sha256::new();
    hasher.update(b"aw-extension-secret-blob-v1\0");
    hasher.update(scope.session_id.as_bytes());
    hasher.update([0]);
    hasher.update(scope.branch_id.as_bytes());
    hasher.update([0]);
    hasher.update(scope.extension.as_bytes());
    hasher.update([0]);
    hasher.update(scope.name.as_bytes());
    hasher.update(nonce);
    Ok(format!("sec_{:x}", hasher.finalize()))
}
fn check_revision(previous: Option<&SecretSettingFact>, expected: Option<u64>) -> Result<u64> {
    if previous.map(|f| f.revision) != expected {
        return Err(Error::Conflict(
            "extension secret revision changed; inspect redacted metadata",
        ));
    }
    previous.map_or(Ok(1), |fact| {
        fact.revision
            .checked_add(1)
            .ok_or(Error::Limit("extension secret revision exhausted"))
    })
}
fn audit_fact(
    c: &rusqlite::Connection,
    owner: &PrincipalId,
    fact: &SecretSettingFact,
) -> Result<()> {
    audit(
        c,
        &fact.scope.session_id,
        owner,
        ACTION,
        serde_json::to_value(fact)
            .map_err(|_| Error::Storage("cannot encode encrypted extension secret fact"))?,
    )
}
impl SessionStore {
    /// Host-only explicit vault SET; `maximum` MUST come from a trusted,
    /// live registered SettingKind::Secret manifest (never a caller's JSON).
    /// Encrypt+sync BEFORE committing a new immutable metadata fact. A process
    /// crash before commit can orphan ciphertext; it never becomes active.
    pub fn extension_secret_set(
        &self,
        owner: &PrincipalId,
        scope: SecretSettingScope,
        expected_revision: Option<u64>,
        maximum: u16,
        secret: &[u8],
    ) -> Result<SecretSettingView> {
        scope.validate()?;
        if maximum == 0
            || usize::from(maximum) > MAX_SECRET
            || secret.is_empty()
            || secret.len() > usize::from(maximum)
        {
            return Err(Error::Limit(
                "extension secret violates registered 1..2048 byte maximum",
            ));
        }
        self.actor_input_writer_fence(owner, &scope.session_id, scope.writer_epoch)?;
        let vault = self.local_secret_store()?;
        let new_ref = random_ref(&scope)?;
        vault.put(&new_ref, secret)?;
        let mutation = (|| -> Result<(SecretSettingView, Option<String>)> {
            let mut conn = self.connection()?;
            let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
            scope_current(&tx, owner, &scope)?;
            let facts = events(&tx, &scope.session_id)?;
            let old = latest(&facts);
            let previous = old.get(&key(&scope));
            let revision = check_revision(previous, expected_revision)?;
            let active = old
                .values()
                .filter(|f| f.state == SecretSettingState::Active)
                .count();
            if previous.is_none_or(|f| f.state != SecretSettingState::Active)
                && active >= MAX_ACTIVE
            {
                return Err(Error::Limit("extension secret active limit reached"));
            }
            if facts.len().saturating_add(1 + RESERVED_CLOSURES) > MAX_AUDIT {
                return Err(Error::Limit(
                    "extension secret facts have no remaining closure budget",
                ));
            }
            let fact = SecretSettingFact {
                scope,
                state: SecretSettingState::Active,
                revision,
                blob_ref: Some(new_ref.clone()),
            };
            audit_fact(&tx, owner, &fact)?;
            db(tx.commit())?;
            Ok((
                SecretSettingView::from(&fact),
                previous.and_then(|f| f.blob_ref.clone()),
            ))
        })();
        match mutation {
            Ok((view, previous)) => {
                // It was committed before any old ciphertext is touched. If
                // unlink/sync fails caller gets uncertainty; owner-only sweep
                // can later remove *ledger-proven retired* refs.
                if let Some(old) = previous {
                    vault.delete(&old)?;
                }
                Ok(view)
            }
            Err(error) => {
                // Failed before a fact was written; best effort cleanup.
                // Power loss may still orphan this unreferenced item; NEVER
                // sweep arbitrary files from the global credentials vault.
                let _ = vault.delete(&new_ref);
                Err(error)
            }
        }
    }
    /// Exact revision-fenced RESET. Facts remain append-only; previous
    /// ciphertext becomes retired only after the new Removed fact commits.
    pub fn extension_secret_reset(
        &self,
        owner: &PrincipalId,
        scope: SecretSettingScope,
        expected_revision: u64,
    ) -> Result<SecretSettingView> {
        scope.validate()?;
        let mut conn = self.connection()?;
        let tx = db(conn.transaction_with_behavior(TransactionBehavior::Immediate))?;
        scope_current(&tx, owner, &scope)?;
        let facts = events(&tx, &scope.session_id)?;
        let current = latest(&facts);
        let previous = current
            .get(&key(&scope))
            .ok_or(Error::Denied("extension secret is not configured"))?;
        if previous.state != SecretSettingState::Active {
            return Err(Error::Conflict("extension secret already removed"));
        }
        let revision = check_revision(Some(previous), Some(expected_revision))?;
        if facts.len() >= MAX_AUDIT {
            return Err(Error::Limit("extension secret closure audit is full"));
        }
        let retired = previous.blob_ref.clone();
        let fact = SecretSettingFact {
            scope,
            state: SecretSettingState::Removed,
            revision,
            blob_ref: None,
        };
        audit_fact(&tx, owner, &fact)?;
        db(tx.commit())?;
        if let Some(old) = retired {
            self.local_secret_store()?.delete(&old)?;
        }
        Ok(SecretSettingView::from(&fact))
    }
    /// View redacted metadata only. Owner/writer/branch/site are checked
    /// independently on EVERY call; no raw refs, digest or value are returned.
    pub fn extension_secret_view(
        &self,
        owner: &PrincipalId,
        scope: &SecretSettingScope,
    ) -> Result<Option<SecretSettingView>> {
        scope.validate()?;
        let c = self.connection()?;
        scope_current(&c, owner, scope)?;
        Ok(latest(&events(&c, &scope.session_id)?)
            .get(&key(scope))
            .map(SecretSettingView::from))
    }
    pub fn extension_secret_list(
        &self,
        owner: &PrincipalId,
        scope: &SecretSettingScope,
    ) -> Result<Vec<SecretSettingView>> {
        scope.validate()?;
        let c = self.connection()?;
        scope_current(&c, owner, scope)?;
        let views = latest(&events(&c, &scope.session_id)?)
            .into_values()
            .filter(|f| f.scope.branch_id == scope.branch_id && f.scope.site == scope.site)
            .map(|f| SecretSettingView::from(&f))
            .collect::<Vec<_>>();
        if views.len() > MAX_ACTIVE {
            return Err(Error::Limit("extension secret metadata list too large"));
        }
        Ok(views)
    }
    /// Explicit private owner READ, never in a model/Hook/UI metadata/RPC
    /// projection. The recovered plaintext is Zeroizing and not Serializable.
    pub fn extension_secret_read(
        &self,
        owner: &PrincipalId,
        scope: &SecretSettingScope,
    ) -> Result<Zeroizing<Vec<u8>>> {
        scope.validate()?;
        let c = self.connection()?;
        scope_current(&c, owner, scope)?;
        let latest = latest(&events(&c, &scope.session_id)?);
        let fact = latest
            .get(&key(scope))
            .ok_or(Error::Denied("extension secret not configured"))?;
        if fact.state != SecretSettingState::Active {
            return Err(Error::Denied("extension secret was removed"));
        }
        let reference = fact
            .blob_ref
            .as_deref()
            .ok_or(Error::Storage("active extension secret lost custody ref"))?;
        // Hold current verified SQLite owner guard while resolving the file.
        // Cross-process changes must not expose a superseded secret via stale
        // read; owner lock and writer fence protect same-host ownership.
        self.local_secret_store()?.get(reference)
    }
    /// Bounded crash-recovery unlink of *only* immutable-ledger-proven retired
    /// blobs in this exact Session/branch/site. Unknown unreferenced orphan
    /// files are deliberately left alone to avoid deleting other credentials.
    pub fn extension_secret_reclaim_retired(
        &self,
        owner: &PrincipalId,
        scope: &SecretSettingScope,
        limit: usize,
    ) -> Result<usize> {
        scope.validate()?;
        if !(1..=MAX_SWEEP).contains(&limit) {
            return Err(Error::Limit(
                "extension custody reclaim limit must be 1..64",
            ));
        }
        let c = self.connection()?;
        scope_current(&c, owner, scope)?;
        let all = events(&c, &scope.session_id)?;
        let protected = latest(&all)
            .values()
            .filter(|f| f.state == SecretSettingState::Active)
            .filter_map(|f| f.blob_ref.clone())
            .collect::<BTreeSet<_>>();
        let retired = all
            .iter()
            .filter(|f| f.scope.branch_id == scope.branch_id && f.scope.site == scope.site)
            .filter_map(|f| f.blob_ref.clone())
            .filter(|reference| !protected.contains(reference))
            .collect::<BTreeSet<_>>();
        let vault = self.local_secret_store()?;
        let mut removed = 0usize;
        for reference in retired.into_iter().take(limit) {
            // Session owner writer is still fenced under current connection.
            if vault.exists(&reference)? {
                vault.delete(&reference)?;
                removed += 1;
            }
        }
        // Audit only the count/scope, not secret refs/contents; operations
        // are idempotent, and a crash before audit never restores ciphertext.
        if removed > 0 {
            audit(
                &c,
                &scope.session_id,
                owner,
                CUSTODY_ACTION,
                serde_json::json!({
                    "branch_id":scope.branch_id,"site":scope.site,"count":removed,
                }),
            )?;
        }
        Ok(removed)
    }
}
