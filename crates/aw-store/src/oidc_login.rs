use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OidcProviderInfo {
    pub provider_id: String,
    pub issuer: String,
    pub client_id: String,
    pub client_auth_method: String,
    pub scopes: BTreeSet<String>,
    pub status: String,
    pub client_secret_version: Option<u64>,
    pub created_by_user_id: String,
    pub created_at: i64,
    pub rotated_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedOidcLogin {
    pub identity: OidcIdentityInfo,
    pub user: StableUserInfo,
    pub newly_created_user: bool,
    pub newly_verified_binding: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct OidcProviderRow {
    pub info: OidcProviderInfo,
    pub client_secret_ref: Option<String>,
}

fn oidc_provider_row(c: &Connection, provider_id: &str) -> Result<OidcProviderRow> {
    let row:(String,String,String,String,String,String,Option<String>,Option<u64>,String,i64,Option<i64>,Option<i64>)=db(c.query_row(
        "SELECT provider_id,issuer,client_id,client_auth_method,scopes_json,status,client_secret_ref,client_secret_version,created_by_user_id,created_at,rotated_at,revoked_at FROM oidc_providers WHERE provider_id=?1",
        [provider_id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?)),
    ).optional())?.ok_or(Error::Denied("OIDC provider unavailable"))?;
    Ok(OidcProviderRow {
        info: OidcProviderInfo {
            provider_id: row.0,
            issuer: row.1,
            client_id: row.2,
            client_auth_method: row.3,
            scopes: decode(&row.4)?,
            status: row.5,
            client_secret_version: row.7,
            created_by_user_id: row.8,
            created_at: row.9,
            rotated_at: row.10,
            revoked_at: row.11,
        },
        client_secret_ref: row.6,
    })
}

fn canonical_issuer(value: &str) -> Result<String> {
    validate_oidc_component(value, 512, "issuer")?;
    let mut url = url::Url::parse(value)
        .map_err(|_| Error::Config("OIDC issuer must be an absolute URL".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::Config("OIDC issuer requires a host".into()))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Denied(
            "OIDC issuer cannot contain credentials, query parameters or fragments",
        ));
    }
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(Error::Denied(
            "OIDC issuer requires HTTPS except loopback development issuers",
        ));
    }
    if url.path().len() > 1 && url.path().ends_with('/') {
        let trimmed = url.path().trim_end_matches('/').to_owned();
        url.set_path(&trimmed);
    }
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn valid_scopes(scopes: &BTreeSet<String>) -> Result<()> {
    if scopes.is_empty() || scopes.len() > 32 || !scopes.contains("openid") {
        return Err(Error::Config(
            "OIDC scopes require openid and at most 32 bounded scopes".into(),
        ));
    }
    if scopes.iter().any(|s| {
        s.is_empty()
            || s.len() > 128
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-/".contains(&b))
    }) {
        return Err(Error::Config("invalid OIDC scope".into()));
    }
    Ok(())
}

fn validate_client_auth(method: &str, secret: Option<&[u8]>) -> Result<()> {
    if !matches!(
        method,
        "none" | "client_secret_post" | "client_secret_basic"
    ) {
        return Err(Error::Config(
            "OIDC client auth must be none, client_secret_post or client_secret_basic".into(),
        ));
    }
    if (method == "none") != secret.is_none() {
        return Err(Error::Config(
            "OIDC client secret presence does not match client auth method".into(),
        ));
    }
    if let Some(secret) = secret {
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(Error::Limit(
                "OIDC client secret must contain 1..65536 bytes",
            ));
        }
    }
    Ok(())
}

pub(crate) fn oidc_client_secret_ref(
    store_id: &str,
    provider_id: &str,
    version: u64,
) -> Result<String> {
    label(provider_id, 128)?;
    if version == 0 {
        return Err(Error::Config(
            "OIDC client-secret version must be positive".into(),
        ));
    }
    let mut h = Sha256::new();
    h.update(b"aw-oidc-client-secret-v1\0");
    h.update(store_id.as_bytes());
    h.update([0]);
    h.update(provider_id.as_bytes());
    h.update([0]);
    h.update(version.to_be_bytes());
    Ok(format!("sec_{:x}", h.finalize()))
}

impl SessionStore {
    pub fn create_oidc_provider(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        provider_id: &str,
        issuer: &str,
        client_id: &str,
        client_auth_method: &str,
        scopes: BTreeSet<String>,
        client_secret: Option<&[u8]>,
    ) -> Result<OidcProviderInfo> {
        label(provider_id, 128)?;
        let issuer = canonical_issuer(issuer)?;
        label(client_id, 256)?;
        valid_scopes(&scopes)?;
        validate_client_auth(client_auth_method, client_secret)?;
        let (user, store_id) = {
            let c = self.connection()?;
            let user = ensure_user_for_principal(&c, actor)?;
            let exists: bool = db(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM oidc_providers WHERE provider_id=?1 OR issuer=?2)",
                params![provider_id, issuer],
                |r| r.get(0),
            ))?;
            if exists {
                return Err(Error::Conflict("OIDC provider ID or issuer already exists"));
            }
            let store_id: String = db(c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            ))?;
            (user, store_id)
        };
        let version = client_secret.map(|_| 1u64);
        let secret_ref = version
            .map(|v| oidc_client_secret_ref(&store_id, provider_id, v))
            .transpose()?;
        if let (Some(r), Some(secret)) = (secret_ref.as_deref(), client_secret) {
            secrets.put(r, secret)?;
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("OIDC provider creator is unavailable"))?;
        if current != user {
            drop(tx);
            if let Some(r) = secret_ref.as_deref() {
                let _ = secrets.delete(r);
            }
            return Err(Error::Denied("OIDC provider creator identity changed"));
        }
        let at = now();
        let result = (|| -> Result<()> {
            db(tx.execute("INSERT INTO oidc_providers(provider_id,issuer,client_id,client_auth_method,scopes_json,status,client_secret_ref,client_secret_version,created_by_user_id,created_at) VALUES(?1,?2,?3,?4,?5,'active',?6,?7,?8,?9)",params![provider_id,issuer,client_id,client_auth_method,json(&scopes)?,secret_ref,version,user,at]))?;
            if let (Some(r), Some(v)) = (secret_ref.as_deref(), version) {
                db(tx.execute("INSERT INTO oidc_provider_secret_versions(provider_id,version,secret_ref,status,created_by_user_id,created_at) VALUES(?1,?2,?3,'active',?4,?5)",params![provider_id,v,r,user,at]))?;
            }
            security_audit(
                &tx,
                &user,
                "oidc.provider_created",
                "oidc_provider",
                Some(provider_id),
                serde_json::json!({"issuer":issuer,"client_auth_method":client_auth_method,"client_secret_version":version,"scopes":scopes}),
            )?;
            Ok(())
        })();
        if let Err(e) = result {
            drop(tx);
            if let Some(r) = secret_ref.as_deref() {
                let _ = secrets.delete(r);
            }
            return Err(e);
        }
        if db(tx.commit()).is_err() {
            if let Some(r) = secret_ref.as_deref() {
                let _ = secrets.delete(r);
            }
            return Err(Error::Storage("OIDC provider creation did not commit"));
        }
        Ok(oidc_provider_row(&c, provider_id)?.info)
    }

    pub fn rotate_oidc_provider_secret(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        provider_id: &str,
        secret: &[u8],
    ) -> Result<OidcProviderInfo> {
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(Error::Limit(
                "OIDC client secret must contain 1..65536 bytes",
            ));
        }
        let (admin, old_version, old_ref, store_id) = {
            let c = self.connection()?;
            let user = user_for_principal(&c, actor)?
                .ok_or(Error::Denied("OIDC provider administrator unavailable"))?;
            let row = oidc_provider_row(&c, provider_id)?;
            if row.info.created_by_user_id != user
                || row.info.status == "revoked"
                || row.info.client_auth_method == "none"
            {
                return Err(Error::Denied(
                    "OIDC provider secret rotation is not permitted",
                ));
            }
            let version = row
                .info
                .client_secret_version
                .ok_or(Error::Denied("OIDC client secret version unavailable"))?;
            let secret_ref = row
                .client_secret_ref
                .clone()
                .ok_or(Error::Denied("OIDC client secret ref unavailable"))?;
            let store_id: String = db(c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |r| r.get(0),
            ))?;
            (user, version, secret_ref, store_id)
        };
        let new_version = old_version
            .checked_add(1)
            .ok_or(Error::Limit("OIDC client secret version exhausted"))?;
        let new_ref = oidc_client_secret_ref(&store_id, provider_id, new_version)?;
        secrets.put(&new_ref, secret)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = oidc_provider_row(&tx, provider_id)?;
        if current.info.created_by_user_id != admin
            || current.info.client_secret_version != Some(old_version)
            || current.client_secret_ref.as_deref() != Some(old_ref.as_str())
            || current.info.status == "revoked"
        {
            drop(tx);
            let _ = secrets.delete(&new_ref);
            return Err(Error::Conflict(
                "OIDC provider secret changed during rotation",
            ));
        }
        let at = now();
        let result = (|| -> Result<()> {
            db(tx.execute(
                "UPDATE oidc_provider_secret_versions SET status='superseded',retired_at=?1 WHERE provider_id=?2 AND version=?3 AND secret_ref=?4 AND status IN ('active','unavailable')",
                params![at,provider_id,old_version,old_ref],
            ))?;
            db(tx.execute(
                "INSERT INTO oidc_provider_secret_versions(provider_id,version,secret_ref,status,created_by_user_id,created_at) VALUES(?1,?2,?3,'active',?4,?5)",
                params![provider_id,new_version,new_ref,admin,at],
            ))?;
            let changed = db(tx.execute(
                "UPDATE oidc_providers SET client_secret_ref=?1,client_secret_version=?2,status='active',rotated_at=?3 WHERE provider_id=?4 AND client_secret_ref=?5 AND client_secret_version=?6 AND status<>'revoked'",
                params![new_ref,new_version,at,provider_id,old_ref,old_version],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "OIDC provider secret changed during rotation",
                ));
            }
            security_audit(
                &tx,
                &admin,
                "oidc.client_secret_rotated",
                "oidc_provider",
                Some(provider_id),
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
                "OIDC provider secret rotation did not commit",
            ));
        }
        let _ = secrets.delete(&old_ref);
        Ok(oidc_provider_row(&c, provider_id)?.info)
    }

    pub fn revoke_oidc_provider(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        provider_id: &str,
    ) -> Result<OidcProviderInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("OIDC provider administrator unavailable"))?;
        let row = oidc_provider_row(&tx, provider_id)?;
        if row.info.created_by_user_id != user {
            return Err(Error::Denied(
                "OIDC provider belongs to another administrator",
            ));
        }
        let at = now();
        db(tx.execute(
            "UPDATE oidc_providers SET status='revoked',revoked_at=COALESCE(revoked_at,?1) WHERE provider_id=?2",
            params![at,provider_id],
        ))?;
        db(tx.execute(
            "UPDATE oidc_provider_secret_versions SET status='revoked',retired_at=COALESCE(retired_at,?1) WHERE provider_id=?2",
            params![at,provider_id],
        ))?;
        security_audit(
            &tx,
            &user,
            "oidc.provider_revoked",
            "oidc_provider",
            Some(provider_id),
            serde_json::json!({"client_secret_version":row.info.client_secret_version}),
        )?;
        db(tx.commit())?;
        if let Some(r) = row.client_secret_ref {
            let _ = secrets.delete(&r);
        }
        Ok(oidc_provider_row(&c, provider_id)?.info)
    }

    pub fn oidc_providers(&self, actor: &PrincipalId) -> Result<Vec<OidcProviderInfo>> {
        let c = self.connection()?;
        let user =
            user_for_principal(&c, actor)?.ok_or(Error::Denied("OIDC providers unavailable"))?;
        let mut q=db(c.prepare("SELECT provider_id FROM oidc_providers WHERE created_by_user_id=?1 ORDER BY provider_id LIMIT 128"))?;
        let ids = db(db(q.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter()
            .map(|id| oidc_provider_row(&c, id).map(|r| r.info))
            .collect()
    }
    pub fn oidc_provider_for_login(&self, provider_id: &str) -> Result<OidcProviderInfo> {
        let c = self.connection()?;
        let row = oidc_provider_row(&c, provider_id)?;
        if row.info.status != "active" {
            return Err(Error::Denied("OIDC provider is not active"));
        }
        Ok(row.info)
    }
    pub fn resolve_oidc_client_secret(
        &self,
        provider_id: &str,
        secrets: &dyn SecretStore,
    ) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
        let c = self.connection()?;
        let row = oidc_provider_row(&c, provider_id)?;
        if row.info.status != "active" {
            return Err(Error::Denied("OIDC provider is not active"));
        }
        match (&row.client_secret_ref, row.info.client_secret_version) {
            (None, None) => Ok(None),
            (Some(r), Some(v)) => {
                let active:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM oidc_provider_secret_versions WHERE provider_id=?1 AND version=?2 AND secret_ref=?3 AND status='active')",params![provider_id,v,r],|x|x.get(0)))?;
                if !active {
                    return Err(Error::Denied("OIDC client secret version is not active"));
                }
                Ok(Some(secrets.get(r)?))
            }
            _ => Err(Error::Storage(
                "OIDC provider client-secret metadata is inconsistent",
            )),
        }
    }

    pub fn complete_verified_oidc(
        &self,
        provider_id: &str,
        issuer: &str,
        subject: &str,
        target_user_id: Option<&str>,
    ) -> Result<VerifiedOidcLogin> {
        let issuer = canonical_issuer(issuer)?;
        validate_oidc_component(subject, 512, "subject")?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let provider = oidc_provider_row(&tx, provider_id)?;
        if provider.info.status != "active" || provider.info.issuer != issuer {
            return Err(Error::Denied(
                "verified OIDC issuer does not match active provider configuration",
            ));
        }
        let external_key = oidc_binding_key(&issuer, subject)?;
        let existing:Option<(String,String,String,Option<i64>)>=db(tx.query_row("SELECT identity_id,user_id,status,login_verified_at FROM user_identities WHERE external_key=?1",[&external_key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional())?;
        if let Some((identity_id, user_id, status, login_verified_at)) = existing {
            if status != "active" {
                return Err(Error::Denied("OIDC identity was explicitly revoked"));
            }
            if let Some(target) = target_user_id {
                if target != user_id {
                    return Err(Error::Conflict(
                        "verified OIDC identity belongs to another stable user",
                    ));
                }
                if login_verified_at.is_none() {
                    let epoch: u64 = db(tx
                        .query_row(
                            "SELECT security_epoch FROM users WHERE user_id=?1 AND status='active'",
                            [&user_id],
                            |r| r.get(0),
                        )
                        .optional())?
                    .ok_or(Error::Denied("OIDC link target user unavailable"))?;
                    let next = epoch
                        .checked_add(1)
                        .ok_or(Error::Limit("user security epoch exhausted"))?;
                    db(tx.execute(
                        "UPDATE users SET security_epoch=?1 WHERE user_id=?2",
                        params![next, user_id],
                    ))?;
                    let verified_at = now();
                    db(tx.execute("UPDATE user_identities SET provider_id=?1,verified_at=?2,login_verified_at=?2 WHERE identity_id=?3",params![provider_id,verified_at,identity_id]))?;
                    security_audit(
                        &tx,
                        &user_id,
                        "identity.oidc_login_verified",
                        "user_identity",
                        Some(&identity_id),
                        serde_json::json!({"provider_id":provider_id,"issuer":issuer,"subject_sha256":format!("{:x}",Sha256::digest(subject.as_bytes())),"linked_existing_user":true}),
                    )?;
                    db(tx.commit())?;
                    return Ok(VerifiedOidcLogin {
                        identity: oidc_row(&c, &identity_id)?,
                        user: user_row(&c, &user_id)?,
                        newly_created_user: false,
                        newly_verified_binding: true,
                    });
                }
            } else if login_verified_at.is_none() {
                return Err(Error::Denied("OIDC subject exists only as manual metadata; explicitly link it while already authenticated"));
            }
            db(tx.commit())?;
            return Ok(VerifiedOidcLogin {
                identity: oidc_row(&c, &identity_id)?,
                user: user_row(&c, &user_id)?,
                newly_created_user: false,
                newly_verified_binding: false,
            });
        }
        let (user_id, new_user) = match target_user_id {
            Some(target) => {
                let active: bool = db(tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM users WHERE user_id=?1 AND status='active')",
                    [target],
                    |r| r.get(0),
                ))?;
                if !active {
                    return Err(Error::Denied("OIDC link target user unavailable"));
                }
                (target.to_owned(), false)
            }
            None => {
                let user = id("usr")?;
                db(tx.execute("INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES(?1,?2,'active',?3,1,?4)",params![user,format!("oidc:{provider_id}"),json(&serde_json::json!({"oidc_provider":provider_id}))?,now()]))?;
                (user, true)
            }
        };
        let identity_id = id("ident")?;
        let at = now();
        db(tx.execute("INSERT INTO user_identities(identity_id,user_id,kind,provider_id,issuer,subject,external_key,verified_at,status,login_verified_at) VALUES(?1,?2,'oidc',?3,?4,?5,?6,?7,'active',?7)",params![identity_id,user_id,provider_id,issuer,subject,external_key,at]))?;
        if !new_user {
            let epoch: u64 = db(tx.query_row(
                "SELECT security_epoch FROM users WHERE user_id=?1",
                [&user_id],
                |r| r.get(0),
            ))?;
            db(tx.execute(
                "UPDATE users SET security_epoch=?1 WHERE user_id=?2",
                params![
                    epoch
                        .checked_add(1)
                        .ok_or(Error::Limit("user security epoch exhausted"))?,
                    user_id
                ],
            ))?;
        }
        security_audit(
            &tx,
            &user_id,
            "identity.oidc_login_verified",
            "user_identity",
            Some(&identity_id),
            serde_json::json!({"provider_id":provider_id,"issuer":issuer,"subject_sha256":format!("{:x}",Sha256::digest(subject.as_bytes())),"new_user":new_user}),
        )?;
        db(tx.commit())?;
        Ok(VerifiedOidcLogin {
            identity: oidc_row(&c, &identity_id)?,
            user: user_row(&c, &user_id)?,
            newly_created_user: new_user,
            newly_verified_binding: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scopes() -> BTreeSet<String> {
        BTreeSet::from(["openid".into(), "profile".into()])
    }

    #[test]
    fn manual_oidc_metadata_is_not_login_until_verified_link() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let user = store.current_user(&actor).unwrap();
        let issuer = "http://127.0.0.1:7777";
        store
            .create_oidc_provider(
                &actor,
                &secrets,
                "fixture",
                issuer,
                "client",
                "none",
                scopes(),
                None,
            )
            .unwrap();
        let manual = store
            .link_oidc_identity(&actor, Some("fixture"), issuer, "subject-a")
            .unwrap();
        assert!(store
            .complete_verified_oidc("fixture", issuer, "subject-a", None)
            .is_err());
        let linked = store
            .complete_verified_oidc("fixture", issuer, "subject-a", Some(&user.user_id))
            .unwrap();
        assert_eq!(linked.user.user_id, user.user_id);
        assert_eq!(linked.identity.identity_id, manual.identity_id);
        assert!(linked.newly_verified_binding);
        let issued = store
            .issue_auth_session_from_verified_oidc(
                &linked.identity.identity_id,
                600,
                serde_json::json!({"client":"oidc-test"}),
            )
            .unwrap();
        assert!(store
            .authenticate_auth_session(&issued.refresh_token)
            .is_ok());
        store
            .revoke_oidc_identity(&actor, &linked.identity.identity_id)
            .unwrap();
        assert!(store
            .authenticate_auth_session(&issued.refresh_token)
            .is_err());
    }

    #[test]
    fn verified_oidc_login_creates_stable_user_once() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let admin = PrincipalId::new("unix:1000").unwrap();
        let issuer = "http://127.0.0.1:7788/tenant";
        let provider = store
            .create_oidc_provider(
                &admin,
                &secrets,
                "tenant",
                issuer,
                "client",
                "none",
                scopes(),
                None,
            )
            .unwrap();
        assert_eq!(provider.issuer, issuer);
        let first = store
            .complete_verified_oidc("tenant", issuer, "subject-new", None)
            .unwrap();
        assert!(first.newly_created_user);
        let second = store
            .complete_verified_oidc("tenant", issuer, "subject-new", None)
            .unwrap();
        assert!(!second.newly_created_user);
        assert_eq!(first.user.user_id, second.user.user_id);
        assert_eq!(first.identity.identity_id, second.identity.identity_id);
    }

    #[test]
    fn oidc_provider_secret_is_versioned_and_revocable() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let admin = PrincipalId::new("unix:1000").unwrap();
        let created = store
            .create_oidc_provider(
                &admin,
                &secrets,
                "secret-provider",
                "https://login.example.invalid/tenant/",
                "client",
                "client_secret_post",
                BTreeSet::from(["openid".into()]),
                Some(b"secret-v1"),
            )
            .unwrap();
        assert_eq!(created.issuer, "https://login.example.invalid/tenant");
        assert_eq!(created.client_secret_version, Some(1));
        assert_eq!(
            store
                .resolve_oidc_client_secret("secret-provider", &secrets)
                .unwrap()
                .unwrap()
                .as_slice(),
            b"secret-v1"
        );
        let rotated = store
            .rotate_oidc_provider_secret(&admin, &secrets, "secret-provider", b"secret-v2")
            .unwrap();
        assert_eq!(rotated.client_secret_version, Some(2));
        assert_eq!(
            store
                .resolve_oidc_client_secret("secret-provider", &secrets)
                .unwrap()
                .unwrap()
                .as_slice(),
            b"secret-v2"
        );
        store
            .revoke_oidc_provider(&admin, &secrets, "secret-provider")
            .unwrap();
        assert!(store
            .resolve_oidc_client_secret("secret-provider", &secrets)
            .is_err());
    }
}
