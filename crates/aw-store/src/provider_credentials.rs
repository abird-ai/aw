use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, net::IpAddr};
use url::Url;
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderAccountInfo {
    pub id: String,
    pub provider_kind: String,
    pub account_owner: String,
    pub payer: String,
    pub credential_custody: String,
    pub executor_id: String,
    pub allowed_models: BTreeSet<String>,
    pub policy: serde_json::Value,
    pub status: String,
    pub secret_version: Option<u64>,
    pub created_at: i64,
    pub rotated_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub grant_role: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderAccountGrantInfo {
    pub provider_account_id: String,
    pub user_id: String,
    pub role: String,
    pub status: String,
    pub revision: u64,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderSecretVersionInfo {
    pub provider_account_id: String,
    pub version: u64,
    pub status: String,
    pub created_by_user_id: Option<String>,
    pub created_at: i64,
    pub retired_at: Option<i64>,
}

#[derive(Debug)]
pub struct ResolvedProviderSecret {
    pub account: ProviderAccountInfo,
    pub secret: Zeroizing<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct ProviderAccountRow {
    info: ProviderAccountInfo,
    secret_ref: Option<String>,
}

fn portable_id(value: &str, what: &'static str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b":._-@".contains(&c))
    {
        return Err(Error::Config(format!("invalid {what}")));
    }
    Ok(())
}

fn validate_provider_kind(kind: &str) -> Result<()> {
    if !matches!(kind, "openai" | "ollama" | "anthropic" | "gemini") {
        return Err(Error::Config(
            "provider kind must be openai, ollama, anthropic or gemini".into(),
        ));
    }
    Ok(())
}

fn validate_server_secret(secret: &[u8]) -> Result<()> {
    if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
        return Err(Error::Limit("provider secret must contain 1..65536 bytes"));
    }
    let text = std::str::from_utf8(secret)
        .map_err(|_| Error::Config("provider bearer credential must be UTF-8".into()))?;
    if text.trim().is_empty() || text.contains('\r') || text.contains('\n') {
        return Err(Error::Config(
            "provider bearer credential is empty or contains a line break".into(),
        ));
    }
    Ok(())
}

fn canonical_base_url(value: &str) -> Result<String> {
    if value.trim().is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        return Err(Error::Config(
            "provider account base_url must be a bounded printable URL selected by the administrator".into(),
        ));
    }
    let mut url = Url::parse(value)
        .map_err(|_| Error::Config("provider account base_url must be an absolute URL".into()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Denied(
            "provider account base_url cannot contain credentials, query parameters or fragments",
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::Config("provider account base_url requires a host".into()))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => {
            return Err(Error::Denied(
                "shared provider credentials require HTTPS; plaintext HTTP is allowed only for loopback development endpoints",
            ))
        }
    }
    // Normalize a trailing slash only; path prefixes such as /v1 remain pinned.
    if url.path().len() > 1 && url.path().ends_with('/') {
        let trimmed = url.path().trim_end_matches('/').to_owned();
        url.set_path(&trimmed);
    }
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn account_base_url(policy: &serde_json::Value) -> Result<&str> {
    policy
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::Storage(
            "provider account policy is missing its pinned base_url",
        ))
}

fn validate_models(models: &BTreeSet<String>) -> Result<()> {
    if models.is_empty() || models.len() > 128 {
        return Err(Error::Config(
            "provider account needs 1..128 explicitly allowed models".into(),
        ));
    }
    if models.iter().any(|model| {
        model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control)
    }) {
        return Err(Error::Config("invalid allowed provider model".into()));
    }
    Ok(())
}

fn provider_audit(
    c: &Connection,
    actor_user_id: &str,
    action: &str,
    account_id: &str,
    outcome: &str,
    credential_handle_id: Option<&str>,
    metadata: serde_json::Value,
) -> Result<()> {
    if !matches!(outcome, "allowed" | "denied" | "failed") {
        return Err(Error::Config("invalid security-audit outcome".into()));
    }
    let store: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |r| r.get(0),
    ))?;
    db(c.execute(
        "INSERT INTO security_audit_events(audit_id,store_id,realm_id,actor_user_id,action,resource_type,resource_id,outcome,credential_handle_id,metadata_json,created_at) SELECT ?1,?2,u.realm_id,?3,?4,'provider_account',?5,?6,?7,?8,?9 FROM users u WHERE u.user_id=?3",
        params![id("audit")?,store,actor_user_id,action,account_id,outcome,credential_handle_id,json(&metadata)?,now()],
    ))?;
    Ok(())
}

fn grant_row(c: &Connection, account_id: &str, user_id: &str) -> Result<ProviderAccountGrantInfo> {
    db(c.query_row(
        "SELECT provider_account_id,user_id,role,status,revision,created_at,revoked_at FROM provider_account_grants WHERE provider_account_id=?1 AND user_id=?2",
        params![account_id,user_id],
        |r|Ok(ProviderAccountGrantInfo{provider_account_id:r.get(0)?,user_id:r.get(1)?,role:r.get(2)?,status:r.get(3)?,revision:r.get(4)?,created_at:r.get(5)?,revoked_at:r.get(6)?}),
    ).optional())?.ok_or(Error::Denied("provider-account grant unavailable"))
}

fn account_row(
    c: &Connection,
    account_id: &str,
    grant_user: Option<&str>,
) -> Result<ProviderAccountRow> {
    let row:(String,String,String,String,String,String,String,String,Option<String>,Option<u64>,i64,Option<i64>,Option<i64>,Option<String>)=db(c.query_row(
        "SELECT p.provider_account_id,p.provider_kind,p.account_owner,p.payer,p.credential_custody,p.executor_id,p.allowed_models_json,p.policy_json,p.secret_ref,p.secret_version,p.created_at,p.rotated_at,p.revoked_at,(SELECT role FROM provider_account_grants g WHERE g.provider_account_id=p.provider_account_id AND g.user_id=?2 AND g.status='active') FROM provider_accounts p WHERE p.provider_account_id=?1",
        params![account_id,grant_user],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?)),
    ).optional())?.ok_or(Error::Denied("provider account unavailable"))?;
    let status: String = db(c.query_row(
        "SELECT status FROM provider_accounts WHERE provider_account_id=?1",
        [account_id],
        |r| r.get(0),
    ))?;
    Ok(ProviderAccountRow {
        info: ProviderAccountInfo {
            id: row.0,
            provider_kind: row.1,
            account_owner: row.2,
            payer: row.3,
            credential_custody: row.4,
            executor_id: row.5,
            allowed_models: decode(&row.6)?,
            policy: decode(&row.7)?,
            status,
            secret_version: row.9,
            created_at: row.10,
            rotated_at: row.11,
            revoked_at: row.12,
            grant_role: row.13,
        },
        secret_ref: row.8,
    })
}

fn require_admin(c: &Connection, actor: &PrincipalId, account_id: &str) -> Result<String> {
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "provider-account administration requires an active known user",
    ))?;
    let grant:Option<String>=db(c.query_row("SELECT role FROM provider_account_grants WHERE provider_account_id=?1 AND user_id=?2 AND status='active'",params![account_id,user],|r|r.get(0)).optional())?;
    if grant.as_deref() != Some("admin") {
        return Err(Error::Denied("provider-account admin grant required"));
    }
    Ok(user)
}

fn require_use(c: &Connection, actor: &PrincipalId, account_id: &str) -> Result<String> {
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "provider-account use requires an active known user",
    ))?;
    let allowed:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM provider_account_grants WHERE provider_account_id=?1 AND user_id=?2 AND status='active' AND role IN ('use','admin'))",params![account_id,user],|r|r.get(0)))?;
    if !allowed {
        return Err(Error::Denied(
            "provider-account use grant required; no fallback was used",
        ));
    }
    Ok(user)
}

impl SessionStore {
    #[allow(clippy::too_many_arguments)]
    pub fn create_server_provider_account(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        account_id: &str,
        provider_kind: &str,
        base_url: &str,
        payer: &str,
        executor_id: &str,
        allowed_models: BTreeSet<String>,
        secret: &[u8],
    ) -> Result<ProviderAccountInfo> {
        portable_id(account_id, "provider account ID")?;
        validate_provider_kind(provider_kind)?;
        let base_url = canonical_base_url(base_url)?;
        portable_id(payer, "provider payer ID")?;
        portable_id(executor_id, "provider executor ID")?;
        if executor_id != "local" {
            return Err(Error::Denied(
                "Phase 31 local SecretStore accounts execute only on the local owner",
            ));
        }
        validate_models(&allowed_models)?;
        validate_server_secret(secret)?;

        // Bootstrap/resolve the local stable user and reserve the logical account
        // name before touching SecretStore, but do not hold a DB transaction
        // across encryption/fsync.
        let (actor_user, store_id) = {
            let mut c = self.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let actor_user = ensure_user_for_principal(&tx, actor)?;
            let exists: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE provider_account_id=?1)",
                [account_id],
                |r| r.get(0),
            ))?;
            if exists {
                return Err(Error::Conflict("provider account already exists"));
            }
            let store_id: String = db(tx.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            ))?;
            db(tx.commit())?;
            (actor_user, store_id)
        };

        let version = 1u64;
        let secret_ref = provider_secret_ref(&store_id, account_id, version)?;
        secrets.put(&secret_ref, secret)?;

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current_actor = user_for_principal(&tx, actor)?.ok_or(Error::Denied(
            "provider-account creator is no longer an active known user",
        ))?;
        if current_actor != actor_user {
            drop(tx);
            let _ = secrets.delete(&secret_ref);
            return Err(Error::Denied(
                "provider-account creator identity changed during creation",
            ));
        }
        let exists: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE provider_account_id=?1)",
            [account_id],
            |r| r.get(0),
        ))?;
        if exists {
            drop(tx);
            let _ = secrets.delete(&secret_ref);
            return Err(Error::Conflict("provider account already exists"));
        }
        let created = now();
        let result = (|| -> Result<()> {
            db(tx.execute("INSERT INTO provider_accounts(provider_account_id,owner_user_id,organization_id,provider_kind,account_owner,payer,credential_custody,executor_id,allowed_models_json,policy_json,status,secret_ref,secret_version,created_at,rotated_at,revoked_at) VALUES(?1,NULL,NULL,?2,'server:local',?3,'server_secret',?4,?5,?6,'active',?7,?8,?9,NULL,NULL)",params![account_id,provider_kind,payer,executor_id,json(&allowed_models)?,json(&serde_json::json!({"shared_with_all_authenticated":false,"base_url":base_url}))?,secret_ref,version,created]))?;
            db(tx.execute("INSERT INTO provider_secret_versions(provider_account_id,version,secret_ref,status,created_by_user_id,created_at) VALUES(?1,?2,?3,'active',?4,?5)",params![account_id,version,secret_ref,actor_user,created]))?;
            db(tx.execute("INSERT INTO provider_account_grants(provider_account_id,user_id,role,status,revision,created_at) VALUES(?1,?2,'admin','active',1,?3)",params![account_id,actor_user,created]))?;
            provider_audit(
                &tx,
                &actor_user,
                "provider.credential_created",
                account_id,
                "allowed",
                Some(&secret_ref),
                serde_json::json!({"provider_kind":provider_kind,"base_url":base_url,"payer":payer,"executor_id":executor_id,"secret_version":version,"allowed_models":allowed_models}),
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(tx);
            let _ = secrets.delete(&secret_ref);
            return Err(error);
        }
        if db(tx.commit()).is_err() {
            let _ = secrets.delete(&secret_ref);
            return Err(Error::Storage("provider-account creation did not commit"));
        }
        account_row(&c, account_id, Some(&actor_user)).map(|row| row.info)
    }

    pub fn provider_account_registered(&self, account_id: &str) -> Result<bool> {
        portable_id(account_id, "provider account ID")?;
        let c = self.connection()?;
        db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE provider_account_id=?1)",
            [account_id],
            |r| r.get(0),
        ))
    }

    pub fn provider_accounts(&self, actor: &PrincipalId) -> Result<Vec<ProviderAccountInfo>> {
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?.ok_or(Error::Denied(
            "provider accounts unavailable or not permitted",
        ))?;
        let mut q=db(c.prepare("SELECT provider_account_id FROM provider_account_grants WHERE user_id=?1 AND status='active' ORDER BY provider_account_id LIMIT 256"))?;
        let ids = db(db(q.query_map([&user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter()
            .map(|id| account_row(&c, id, Some(&user)).map(|row| row.info))
            .collect()
    }

    pub fn provider_account_grants(
        &self,
        actor: &PrincipalId,
        account_id: &str,
    ) -> Result<Vec<ProviderAccountGrantInfo>> {
        let c = self.connection()?;
        let _ = require_admin(&c, actor, account_id)?;
        let mut q=db(c.prepare("SELECT user_id FROM provider_account_grants WHERE provider_account_id=?1 ORDER BY user_id LIMIT 1024"))?;
        let users = db(db(q.query_map([account_id], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        users
            .iter()
            .map(|user| grant_row(&c, account_id, user))
            .collect()
    }

    pub fn grant_provider_account(
        &self,
        actor: &PrincipalId,
        account_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<ProviderAccountGrantInfo> {
        if !matches!(role, "use" | "admin") {
            return Err(Error::Config(
                "provider-account role must be use or admin".into(),
            ));
        }
        stable_user_principal(user_id)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let admin = require_admin(&tx, actor, account_id)?;
        let target: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE user_id=?1 AND status='active')",
            [user_id],
            |r| r.get(0),
        ))?;
        if !target {
            return Err(Error::Denied(
                "provider-account grant target user is unavailable",
            ));
        }
        let existing:Option<(String,u64)>=db(tx.query_row("SELECT status,revision FROM provider_account_grants WHERE provider_account_id=?1 AND user_id=?2",params![account_id,user_id],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        match existing {
            Some((_status, revision)) => {
                db(tx.execute("UPDATE provider_account_grants SET role=?1,status='active',revision=?2,revoked_at=NULL WHERE provider_account_id=?3 AND user_id=?4",params![role,revision.saturating_add(1),account_id,user_id]))?;
            }
            None => {
                db(tx.execute("INSERT INTO provider_account_grants(provider_account_id,user_id,role,status,revision,created_at) VALUES(?1,?2,?3,'active',1,?4)",params![account_id,user_id,role,now()]))?;
            }
        }
        provider_audit(
            &tx,
            &admin,
            "provider.grant_updated",
            account_id,
            "allowed",
            None,
            serde_json::json!({"target_user_id":user_id,"role":role}),
        )?;
        db(tx.commit())?;
        grant_row(&c, account_id, user_id)
    }

    pub fn revoke_provider_account_grant(
        &self,
        actor: &PrincipalId,
        account_id: &str,
        user_id: &str,
    ) -> Result<ProviderAccountGrantInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let admin = require_admin(&tx, actor, account_id)?;
        let grant = grant_row(&tx, account_id, user_id)?;
        if grant.status == "active" && grant.role == "admin" {
            let others:u64=db(tx.query_row("SELECT count(*) FROM provider_account_grants WHERE provider_account_id=?1 AND user_id<>?2 AND role='admin' AND status='active'",params![account_id,user_id],|r|r.get(0)))?;
            if others == 0 {
                return Err(Error::Denied(
                    "cannot revoke the last active provider-account admin",
                ));
            }
        }
        if grant.status == "active" {
            db(tx.execute("UPDATE provider_account_grants SET status='revoked',revision=revision+1,revoked_at=?1 WHERE provider_account_id=?2 AND user_id=?3",params![now(),account_id,user_id]))?;
            provider_audit(
                &tx,
                &admin,
                "provider.grant_revoked",
                account_id,
                "allowed",
                None,
                serde_json::json!({"target_user_id":user_id}),
            )?;
        }
        db(tx.commit())?;
        grant_row(&c, account_id, user_id)
    }

    pub fn provider_account_for_use(
        &self,
        actor: &PrincipalId,
        account_id: &str,
        model: &str,
        base_url: &str,
        executor_id: &str,
    ) -> Result<ProviderAccountInfo> {
        let c = self.connection()?;
        let user = require_use(&c, actor, account_id)?;
        let row = account_row(&c, account_id, Some(&user))?;
        if row.info.status != "active" {
            return Err(Error::Denied(
                "selected provider account is unavailable or revoked; no fallback was used",
            ));
        }
        if row.info.credential_custody != "server_secret" || row.info.executor_id != executor_id {
            return Err(Error::Denied(
                "selected provider account belongs to another credential/executor boundary",
            ));
        }
        let base_url = canonical_base_url(base_url)?;
        if account_base_url(&row.info.policy)? != base_url {
            return Err(Error::Denied(
                "selected provider account is pinned to a different endpoint; no credential was released",
            ));
        }
        if !row.info.allowed_models.contains(model) {
            return Err(Error::Denied(
                "selected provider account does not allow this model; no fallback was used",
            ));
        }
        Ok(row.info)
    }

    pub fn resolve_server_provider_secret(
        &self,
        actor: &PrincipalId,
        account_id: &str,
        model: &str,
        base_url: &str,
        executor_id: &str,
        expected_version: u64,
        secrets: &dyn SecretStore,
    ) -> Result<ResolvedProviderSecret> {
        let base_url = canonical_base_url(base_url)?;
        let (user, row, secret_ref) = {
            let c = self.connection()?;
            let user = require_use(&c, actor, account_id)?;
            let row = account_row(&c, account_id, Some(&user))?;
            if row.info.status != "active" {
                return Err(Error::Denied(
                    "selected provider account is unavailable or revoked; no fallback was used",
                ));
            }
            if row.info.credential_custody != "server_secret" || row.info.executor_id != executor_id
            {
                return Err(Error::Denied(
                    "selected provider account belongs to another credential/executor boundary",
                ));
            }
            if account_base_url(&row.info.policy)? != base_url {
                return Err(Error::Denied(
                    "selected provider account is pinned to a different endpoint; no credential was released",
                ));
            }
            if !row.info.allowed_models.contains(model) {
                return Err(Error::Denied(
                    "selected provider account does not allow this model; no fallback was used",
                ));
            }
            if row.info.secret_version != Some(expected_version) {
                return Err(Error::Denied(
                    "provider credential version changed; select the account again explicitly",
                ));
            }
            let secret_ref = row
                .secret_ref
                .clone()
                .ok_or(Error::Denied("provider secret reference unavailable"))?;
            let valid:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM provider_secret_versions WHERE provider_account_id=?1 AND version=?2 AND secret_ref=?3 AND status='active')",params![account_id,expected_version,secret_ref],|r|r.get(0)))?;
            if !valid {
                return Err(Error::Denied("provider credential version is not active"));
            }
            (user, row, secret_ref)
        };
        match secrets.get(&secret_ref) {
            Ok(secret) => {
                let c = self.connection()?;
                provider_audit(
                    &c,
                    &user,
                    "provider.credential_used",
                    account_id,
                    "allowed",
                    Some(&secret_ref),
                    serde_json::json!({"secret_version":expected_version,"model":model,"base_url":base_url,"executor_id":executor_id}),
                )?;
                Ok(ResolvedProviderSecret {
                    account: row.info,
                    secret,
                })
            }
            Err(_) => {
                let mut c = self.connection()?;
                let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
                db(tx.execute("UPDATE provider_accounts SET status='unavailable' WHERE provider_account_id=?1 AND status='active' AND secret_version=?2 AND secret_ref=?3",params![account_id,expected_version,secret_ref]))?;
                db(tx.execute("UPDATE provider_secret_versions SET status='unavailable',retired_at=COALESCE(retired_at,?1) WHERE provider_account_id=?2 AND version=?3 AND secret_ref=?4 AND status='active'",params![now(),account_id,expected_version,secret_ref]))?;
                provider_audit(
                    &tx,
                    &user,
                    "provider.credential_unavailable",
                    account_id,
                    "failed",
                    Some(&secret_ref),
                    serde_json::json!({"secret_version":expected_version,"no_fallback":true}),
                )?;
                db(tx.commit())?;
                Err(Error::Denied(
                    "selected provider credential is unavailable; no fallback was used",
                ))
            }
        }
    }

    pub fn rotate_server_provider_secret(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        account_id: &str,
        secret: &[u8],
    ) -> Result<ProviderAccountInfo> {
        validate_server_secret(secret)?;
        let (admin, old_version, old_ref, store_id) = {
            let c = self.connection()?;
            let admin = require_admin(&c, actor, account_id)?;
            let current = account_row(&c, account_id, Some(&admin))?;
            if current.info.credential_custody != "server_secret"
                || current.info.status == "revoked"
            {
                return Err(Error::Denied(
                    "provider account cannot rotate a server secret",
                ));
            }
            let old_version = current
                .info
                .secret_version
                .ok_or(Error::Denied("provider secret version unavailable"))?;
            let old_ref = current
                .secret_ref
                .clone()
                .ok_or(Error::Denied("provider secret reference unavailable"))?;
            let store_id: String = db(c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            ))?;
            (admin, old_version, old_ref, store_id)
        };
        let new_version = old_version
            .checked_add(1)
            .ok_or(Error::Limit("provider secret version exhausted"))?;
        let new_ref = provider_secret_ref(&store_id, account_id, new_version)?;
        secrets.put(&new_ref, secret)?;

        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current_admin = require_admin(&tx, actor, account_id)?;
        let current = account_row(&tx, account_id, Some(&current_admin))?;
        if current_admin != admin
            || current.info.status == "revoked"
            || current.info.secret_version != Some(old_version)
            || current.secret_ref.as_deref() != Some(old_ref.as_str())
        {
            drop(tx);
            let _ = secrets.delete(&new_ref);
            return Err(Error::Conflict(
                "provider credential changed during rotation; inspect and retry explicitly",
            ));
        }
        let at = now();
        let result = (|| -> Result<()> {
            db(tx.execute("UPDATE provider_secret_versions SET status='superseded',retired_at=?1 WHERE provider_account_id=?2 AND version=?3 AND secret_ref=?4 AND status IN ('active','unavailable')",params![at,account_id,old_version,old_ref]))?;
            db(tx.execute("INSERT INTO provider_secret_versions(provider_account_id,version,secret_ref,status,created_by_user_id,created_at) VALUES(?1,?2,?3,'active',?4,?5)",params![account_id,new_version,new_ref,admin,at]))?;
            let changed = db(tx.execute("UPDATE provider_accounts SET secret_ref=?1,secret_version=?2,status='active',rotated_at=?3,revoked_at=NULL WHERE provider_account_id=?4 AND secret_version=?5 AND secret_ref=?6 AND status<>'revoked'",params![new_ref,new_version,at,account_id,old_version,old_ref]))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "provider credential changed during rotation; inspect and retry explicitly",
                ));
            }
            provider_audit(
                &tx,
                &admin,
                "provider.credential_rotated",
                account_id,
                "allowed",
                Some(&new_ref),
                serde_json::json!({"from_version":old_version,"to_version":new_version}),
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(tx);
            let _ = secrets.delete(&new_ref);
            return Err(error);
        }
        if db(tx.commit()).is_err() {
            let _ = secrets.delete(&new_ref);
            return Err(Error::Storage(
                "provider credential rotation did not commit",
            ));
        }
        let _ = secrets.delete(&old_ref);
        account_row(&c, account_id, Some(&admin)).map(|row| row.info)
    }

    pub fn revoke_server_provider_account(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        account_id: &str,
    ) -> Result<ProviderAccountInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let admin = require_admin(&tx, actor, account_id)?;
        let current = account_row(&tx, account_id, Some(&admin))?;
        let refs = {
            let mut q = db(tx.prepare(
                "SELECT secret_ref FROM provider_secret_versions WHERE provider_account_id=?1",
            ))?;
            let rows = db(q.query_map([account_id], |r| r.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        let at = now();
        db(tx.execute("UPDATE provider_accounts SET status='revoked',revoked_at=COALESCE(revoked_at,?1) WHERE provider_account_id=?2",params![at,account_id]))?;
        db(tx.execute("UPDATE provider_secret_versions SET status='revoked',retired_at=COALESCE(retired_at,?1) WHERE provider_account_id=?2",params![at,account_id]))?;
        provider_audit(
            &tx,
            &admin,
            "provider.credential_revoked",
            account_id,
            "allowed",
            current.secret_ref.as_deref(),
            serde_json::json!({"secret_version":current.info.secret_version}),
        )?;
        db(tx.commit())?;
        for secret_ref in refs {
            let _ = secrets.delete(&secret_ref);
        }
        account_row(&c, account_id, Some(&admin)).map(|row| row.info)
    }

    pub fn provider_secret_versions(
        &self,
        actor: &PrincipalId,
        account_id: &str,
    ) -> Result<Vec<ProviderSecretVersionInfo>> {
        let c = self.connection()?;
        let _ = require_admin(&c, actor, account_id)?;
        let mut q=db(c.prepare("SELECT version,status,created_by_user_id,created_at,retired_at FROM provider_secret_versions WHERE provider_account_id=?1 ORDER BY version"))?;
        let rows = db(q.query_map([account_id], |r| {
            Ok(ProviderSecretVersionInfo {
                provider_account_id: account_id.to_owned(),
                version: r.get(0)?,
                status: r.get(1)?,
                created_by_user_id: r.get(2)?,
                created_at: r.get(3)?,
                retired_at: r.get(4)?,
            })
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://fixture.invalid/v1";

    fn models() -> BTreeSet<String> {
        BTreeSet::from(["fixture".into()])
    }

    #[test]
    fn native_gemini_account_kind_uses_existing_versioned_secret_custody() {
        assert!(validate_provider_kind("gemini").is_ok());
        assert!(validate_provider_kind("unknown-google-host").is_err());
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let created = store
            .create_server_provider_account(
                &actor,
                &secrets,
                "gemini-fixture",
                "gemini",
                BASE,
                "team",
                "local",
                models(),
                b"fake-api-key",
            )
            .unwrap();
        let record = store
            .resolve_server_provider_secret(
                &actor,
                "gemini-fixture",
                "fixture",
                BASE,
                "local",
                created.secret_version.unwrap(),
                &secrets,
            )
            .unwrap();
        assert_eq!(record.account.provider_kind, "gemini");
        assert_eq!(record.secret.as_slice(), b"fake-api-key");
        assert!(store
            .resolve_server_provider_secret(
                &actor,
                "gemini-fixture",
                "fixture",
                "https://other.invalid/v1beta",
                "local",
                created.secret_version.unwrap(),
                &secrets,
            )
            .is_err());
    }

    #[test]
    fn native_anthropic_account_kind_uses_existing_versioned_secret_custody() {
        assert!(validate_provider_kind("anthropic").is_ok());
        assert!(validate_provider_kind("untrusted-unknown").is_err());
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let created = store
            .create_server_provider_account(
                &actor,
                &secrets,
                "anthropic-fixture",
                "anthropic",
                BASE,
                "team",
                "local",
                models(),
                b"fixture-api-key",
            )
            .unwrap();
        let record = store
            .resolve_server_provider_secret(
                &actor,
                "anthropic-fixture",
                "fixture",
                BASE,
                "local",
                created.secret_version.unwrap(),
                &secrets,
            )
            .unwrap();
        assert_eq!(record.account.provider_kind, "anthropic");
        assert_eq!(record.secret.as_slice(), b"fixture-api-key");
        assert!(store
            .resolve_server_provider_secret(
                &actor,
                "anthropic-fixture",
                "fixture",
                "https://other.invalid/v1",
                "local",
                created.secret_version.unwrap(),
                &secrets,
            )
            .is_err());
    }

    #[test]
    fn shared_secret_rotation_is_versioned_and_no_fallback() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let created = store
            .create_server_provider_account(
                &actor,
                &secrets,
                "shared",
                "openai",
                BASE,
                "team",
                "local",
                models(),
                b"key-v1",
            )
            .unwrap();
        assert_eq!(created.secret_version, Some(1));
        let first = store
            .resolve_server_provider_secret(&actor, "shared", "fixture", BASE, "local", 1, &secrets)
            .unwrap();
        assert_eq!(first.secret.as_slice(), b"key-v1");
        assert!(store
            .resolve_server_provider_secret(
                &actor,
                "shared",
                "fixture",
                "https://attacker.invalid/v1",
                "local",
                1,
                &secrets,
            )
            .is_err());
        let rotated = store
            .rotate_server_provider_secret(&actor, &secrets, "shared", b"key-v2")
            .unwrap();
        assert_eq!(rotated.secret_version, Some(2));
        assert!(store
            .resolve_server_provider_secret(&actor, "shared", "fixture", BASE, "local", 1, &secrets)
            .is_err());
        assert_eq!(
            store
                .resolve_server_provider_secret(
                    &actor, "shared", "fixture", BASE, "local", 2, &secrets
                )
                .unwrap()
                .secret
                .as_slice(),
            b"key-v2"
        );
        store
            .revoke_server_provider_account(&actor, &secrets, "shared")
            .unwrap();
        assert!(store
            .resolve_server_provider_secret(&actor, "shared", "fixture", BASE, "local", 2, &secrets)
            .is_err());
    }

    #[test]
    fn missing_secret_marks_only_selected_account_unavailable() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let created = store
            .create_server_provider_account(
                &actor,
                &secrets,
                "shared",
                "openai",
                BASE,
                "team",
                "local",
                models(),
                b"key",
            )
            .unwrap();
        let user_id = store.current_user(&actor).unwrap().user_id;
        let row = {
            let c = store.connection().unwrap();
            account_row(&c, "shared", Some(&user_id)).unwrap()
        };
        secrets.delete(row.secret_ref.as_deref().unwrap()).unwrap();
        assert!(store
            .resolve_server_provider_secret(
                &actor,
                "shared",
                "fixture",
                BASE,
                "local",
                created.secret_version.unwrap(),
                &secrets
            )
            .is_err());
        let info = store.provider_accounts(&actor).unwrap().remove(0);
        assert_eq!(info.status, "unavailable");
    }

    #[test]
    fn use_grants_are_explicit_and_last_admin_is_protected() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let owner = PrincipalId::new("unix:1000").unwrap();
        let other = PrincipalId::new("internal:fixture:other").unwrap();
        let other_user = store.current_user(&other).unwrap();
        store
            .create_server_provider_account(
                &owner,
                &secrets,
                "shared",
                "openai",
                BASE,
                "team",
                "local",
                models(),
                b"key",
            )
            .unwrap();
        assert!(store
            .provider_account_for_use(&other, "shared", "fixture", BASE, "local")
            .is_err());
        store
            .grant_provider_account(&owner, "shared", &other_user.user_id, "use")
            .unwrap();
        assert!(store
            .provider_account_for_use(&other, "shared", "fixture", BASE, "local")
            .is_ok());
        let owner_user = store.current_user(&owner).unwrap();
        assert!(store
            .revoke_provider_account_grant(&owner, "shared", &owner_user.user_id)
            .is_err());
    }
}
