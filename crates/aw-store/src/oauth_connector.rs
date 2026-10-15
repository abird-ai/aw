use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OAuthConnectorInfo {
    pub connector_id: String,
    pub owner_user_id: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub client_id: String,
    pub client_auth_method: String,
    pub requested_scopes: BTreeSet<String>,
    pub granted_scopes: BTreeSet<String>,
    pub status: String,
    pub client_secret_version: Option<u64>,
    pub token_version: Option<u64>,
    pub access_expires_at: Option<i64>,
    pub created_at: i64,
    pub rotated_at: Option<i64>,
    pub revoked_at: Option<i64>,
}
#[derive(Clone, Debug)]
pub(crate) struct OAuthConnectorRow {
    pub info: OAuthConnectorInfo,
    pub client_secret_ref: Option<String>,
    pub token_ref: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthTokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Retained only inside SecretStore when an OAuth/OIDC-backed inference
    /// contract requires re-validation or account identity display.
    #[serde(default)]
    pub id_token: Option<String>,
    /// Exact verified OIDC identity for OAuth-backed accounts that return an
    /// ID token. Generic OAuth connectors leave both fields unset.
    #[serde(default)]
    pub oidc_issuer: Option<String>,
    #[serde(default)]
    pub oidc_subject: Option<String>,
    pub token_type: String,
    pub scopes: BTreeSet<String>,
    pub access_expires_at: Option<i64>,
}
impl Drop for OAuthTokenSet {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(refresh) = self.refresh_token.as_mut() {
            refresh.zeroize();
        }
        if let Some(id_token) = self.id_token.as_mut() {
            id_token.zeroize();
        }
        if let Some(subject) = self.oidc_subject.as_mut() {
            subject.zeroize();
        }
    }
}

fn endpoint(v: &str) -> Result<()> {
    if v.is_empty() || v.len() > 2048 || v.chars().any(char::is_control) {
        return Err(Error::Config("invalid OAuth endpoint".into()));
    }
    let u =
        url::Url::parse(v).map_err(|_| Error::Config("OAuth endpoint must be absolute".into()))?;
    let host = u
        .host_str()
        .ok_or(Error::Config("OAuth endpoint requires a host".into()))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|a| a.is_loopback());
    if !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err(Error::Denied(
            "OAuth endpoint cannot contain credentials/query/fragment",
        ));
    }
    if u.scheme() != "https" && !(u.scheme() == "http" && loopback) {
        return Err(Error::Denied(
            "OAuth endpoints require HTTPS except loopback development endpoints",
        ));
    }
    Ok(())
}
fn scopes(v: &BTreeSet<String>) -> Result<()> {
    if v.is_empty() || v.len() > 64 {
        return Err(Error::Config(
            "OAuth connector requires 1..64 scopes".into(),
        ));
    }
    if v.iter().any(|s| {
        s.is_empty()
            || s.len() > 128
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-/".contains(&b))
    }) {
        return Err(Error::Config("invalid OAuth scope".into()));
    }
    Ok(())
}
fn row(c: &Connection, id: &str) -> Result<OAuthConnectorRow> {
    let r:(String,String,String,String,String,String,String,String,String,Option<String>,Option<u64>,Option<String>,Option<u64>,Option<i64>,i64,Option<i64>,Option<i64>)=db(c.query_row("SELECT connector_id,owner_user_id,authorization_endpoint,token_endpoint,client_id,client_auth_method,requested_scopes_json,granted_scopes_json,status,client_secret_ref,client_secret_version,token_ref,token_version,access_expires_at,created_at,rotated_at,revoked_at FROM oauth_connector_accounts WHERE connector_id=?1",[id],|x|Ok((x.get(0)?,x.get(1)?,x.get(2)?,x.get(3)?,x.get(4)?,x.get(5)?,x.get(6)?,x.get(7)?,x.get(8)?,x.get(9)?,x.get(10)?,x.get(11)?,x.get(12)?,x.get(13)?,x.get(14)?,x.get(15)?,x.get(16)?))).optional())?.ok_or(Error::Denied("OAuth connector unavailable"))?;
    Ok(OAuthConnectorRow {
        info: OAuthConnectorInfo {
            connector_id: r.0,
            owner_user_id: r.1,
            authorization_endpoint: r.2,
            token_endpoint: r.3,
            client_id: r.4,
            client_auth_method: r.5,
            requested_scopes: decode(&r.6)?,
            granted_scopes: decode(&r.7)?,
            status: r.8,
            client_secret_version: r.10,
            token_version: r.12,
            access_expires_at: r.13,
            created_at: r.14,
            rotated_at: r.15,
            revoked_at: r.16,
        },
        client_secret_ref: r.9,
        token_ref: r.11,
    })
}
fn ref_for(store: &str, id: &str, kind: &str, version: u64) -> Result<String> {
    label(id, 128)?;
    if version == 0 {
        return Err(Error::Config(
            "OAuth secret version must be positive".into(),
        ));
    }
    let mut h = Sha256::new();
    h.update(b"aw-oauth-connector-secret-v1\0");
    h.update(store.as_bytes());
    h.update([0]);
    h.update(id.as_bytes());
    h.update([0]);
    h.update(kind.as_bytes());
    h.update([0]);
    h.update(version.to_be_bytes());
    Ok(format!("sec_{:x}", h.finalize()))
}
fn connector_audit(
    c: &Connection,
    user_id: &str,
    action: &str,
    connector_id: &str,
    metadata: serde_json::Value,
) -> Result<()> {
    security_audit(
        c,
        user_id,
        action,
        "oauth_connector",
        Some(connector_id),
        metadata,
    )
}

fn token_bytes(t: &OAuthTokenSet) -> Result<Zeroizing<Vec<u8>>> {
    if !t.token_type.eq_ignore_ascii_case("bearer") {
        return Err(Error::Denied("OAuth token set must use Bearer token type"));
    }
    if t.access_token.is_empty()
        || t.access_token.len() > MAX_SECRET_BYTES
        || t.refresh_token
            .as_ref()
            .is_some_and(|x| x.len() > MAX_SECRET_BYTES)
        || t.id_token
            .as_ref()
            .is_some_and(|x| x.len() > MAX_SECRET_BYTES)
    {
        return Err(Error::Limit("OAuth token value exceeds bound"));
    }
    if t.oidc_issuer.is_some() != t.oidc_subject.is_some() {
        return Err(Error::Denied(
            "OAuth OIDC issuer and subject must be stored together",
        ));
    }
    if t.oidc_issuer.as_ref().is_some_and(|value| {
        value.is_empty() || value.len() > 512 || value.chars().any(char::is_control)
    }) || t.oidc_subject.as_ref().is_some_and(|value| {
        value.is_empty() || value.len() > 512 || value.chars().any(char::is_control)
    }) {
        return Err(Error::Denied("OAuth OIDC identity is invalid"));
    }
    Ok(Zeroizing::new(serde_json::to_vec(t).map_err(|_| {
        Error::Storage("cannot encode OAuth token set")
    })?))
}

impl SessionStore {
    pub fn create_oauth_connector(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        id: &str,
        authorization_endpoint: &str,
        token_endpoint: &str,
        client_id: &str,
        client_auth_method: &str,
        requested_scopes: BTreeSet<String>,
        client_secret: Option<&[u8]>,
    ) -> Result<OAuthConnectorInfo> {
        label(id, 128)?;
        endpoint(authorization_endpoint)?;
        endpoint(token_endpoint)?;
        label(client_id, 256)?;
        scopes(&requested_scopes)?;
        if !matches!(
            client_auth_method,
            "none" | "client_secret_post" | "client_secret_basic"
        ) || (client_auth_method == "none") != client_secret.is_none()
        {
            return Err(Error::Config("OAuth client auth/secret mismatch".into()));
        }
        let (user, store_id) = {
            let c = self.connection()?;
            (
                ensure_user_for_principal(&c, actor)?,
                db(c.query_row(
                    "SELECT value FROM store_meta WHERE key='store_id'",
                    [],
                    |r| r.get::<_, String>(0),
                ))?,
            )
        };
        let sv = client_secret.map(|_| 1u64);
        let sref = sv
            .map(|v| ref_for(&store_id, id, "client", v))
            .transpose()?;
        if let (Some(r), Some(secret)) = (sref.as_deref(), client_secret) {
            secrets.put(r, secret)?;
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current_user = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("OAuth connector creator is unavailable"))?;
        let exists: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM oauth_connector_accounts WHERE connector_id=?1)",
            [id],
            |r| r.get(0),
        ))?;
        if current_user != user || exists {
            drop(tx);
            if let Some(r) = sref.as_deref() {
                let _ = secrets.delete(r);
            }
            return if exists {
                Err(Error::Conflict("OAuth connector already exists"))
            } else {
                Err(Error::Denied("OAuth connector creator identity changed"))
            };
        }
        let at = now();
        let result = (|| -> Result<()> {
            db(tx.execute("INSERT INTO oauth_connector_accounts(connector_id,owner_user_id,authorization_endpoint,token_endpoint,client_id,client_auth_method,requested_scopes_json,status,client_secret_ref,client_secret_version,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'configured',?8,?9,?10)",params![id,user,authorization_endpoint,token_endpoint,client_id,client_auth_method,json(&requested_scopes)?,sref,sv,at]))?;
            if let (Some(r), Some(v)) = (sref.as_deref(), sv) {
                db(tx.execute("INSERT INTO oauth_connector_client_secret_versions(connector_id,version,secret_ref,status,created_at) VALUES(?1,?2,?3,'active',?4)",params![id,v,r,at]))?;
            }
            connector_audit(
                &tx,
                &user,
                "oauth.connector_created",
                id,
                serde_json::json!({"client_auth_method":client_auth_method,"requested_scopes":requested_scopes,"client_secret_version":sv}),
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(tx);
            if let Some(r) = sref.as_deref() {
                let _ = secrets.delete(r);
            }
            return Err(error);
        }
        if db(tx.commit()).is_err() {
            if let Some(r) = sref.as_deref() {
                let _ = secrets.delete(r);
            }
            return Err(Error::Storage("OAuth connector creation did not commit"));
        }
        Ok(row(&c, id)?.info)
    }
    pub fn oauth_connectors(&self, actor: &PrincipalId) -> Result<Vec<OAuthConnectorInfo>> {
        let c = self.connection()?;
        let user =
            user_for_principal(&c, actor)?.ok_or(Error::Denied("OAuth connectors unavailable"))?;
        let mut q=db(c.prepare("SELECT connector_id FROM oauth_connector_accounts WHERE owner_user_id=?1 ORDER BY connector_id LIMIT 256"))?;
        let ids = db(db(q.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| row(&c, id).map(|r| r.info)).collect()
    }
    pub fn oauth_connector_for_owner(
        &self,
        actor: &PrincipalId,
        id: &str,
    ) -> Result<OAuthConnectorInfo> {
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?
            .ok_or(Error::Denied("OAuth connector owner unavailable"))?;
        let r = row(&c, id)?;
        if r.info.owner_user_id != user || r.info.status == "revoked" {
            return Err(Error::Denied(
                "OAuth connector unavailable or not owned by current user",
            ));
        }
        Ok(r.info)
    }
    pub fn resolve_oauth_client_secret(
        &self,
        actor: &PrincipalId,
        id: &str,
        secrets: &dyn SecretStore,
    ) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?
            .ok_or(Error::Denied("OAuth connector owner unavailable"))?;
        let r = row(&c, id)?;
        if r.info.owner_user_id != user || r.info.status == "revoked" {
            return Err(Error::Denied("OAuth connector unavailable"));
        }
        match (&r.client_secret_ref, r.info.client_secret_version) {
            (None, None) => Ok(None),
            (Some(sr), Some(v)) => {
                let active:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM oauth_connector_client_secret_versions WHERE connector_id=?1 AND version=?2 AND secret_ref=?3 AND status='active')",params![id,v,sr],|x|x.get(0)))?;
                if !active {
                    return Err(Error::Denied("OAuth connector client secret is not active"));
                }
                Ok(Some(secrets.get(sr)?))
            }
            _ => Err(Error::Storage(
                "OAuth connector client-secret metadata inconsistent",
            )),
        }
    }
    pub fn rotate_oauth_connector_client_secret(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        id: &str,
        secret: &[u8],
    ) -> Result<OAuthConnectorInfo> {
        if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
            return Err(Error::Limit(
                "OAuth client secret must contain 1..65536 bytes",
            ));
        }
        let (user, old_version, old_ref, store_id) = {
            let c = self.connection()?;
            let user = user_for_principal(&c, actor)?
                .ok_or(Error::Denied("OAuth connector owner unavailable"))?;
            let r = row(&c, id)?;
            if r.info.owner_user_id != user
                || r.info.status == "revoked"
                || r.info.client_auth_method == "none"
            {
                return Err(Error::Denied(
                    "OAuth connector client-secret rotation is not permitted",
                ));
            }
            let old_version = r
                .info
                .client_secret_version
                .ok_or(Error::Denied("OAuth client secret version unavailable"))?;
            let old_ref = r
                .client_secret_ref
                .clone()
                .ok_or(Error::Denied("OAuth client secret ref unavailable"))?;
            let store_id: String = db(c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |x| x.get(0),
            ))?;
            (user, old_version, old_ref, store_id)
        };
        let new_version = old_version
            .checked_add(1)
            .ok_or(Error::Limit("OAuth client secret version exhausted"))?;
        let new_ref = ref_for(&store_id, id, "client", new_version)?;
        secrets.put(&new_ref, secret)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = row(&tx, id)?;
        if current.info.owner_user_id != user
            || current.info.client_secret_version != Some(old_version)
            || current.client_secret_ref.as_deref() != Some(old_ref.as_str())
            || current.info.status == "revoked"
        {
            drop(tx);
            let _ = secrets.delete(&new_ref);
            return Err(Error::Conflict(
                "OAuth connector client secret changed during rotation",
            ));
        }
        let at = now();
        let result = (|| -> Result<()> {
            db(tx.execute(
                "UPDATE oauth_connector_client_secret_versions SET status='superseded',retired_at=?1 WHERE connector_id=?2 AND version=?3 AND secret_ref=?4 AND status IN ('active','unavailable')",
                params![at,id,old_version,old_ref],
            ))?;
            db(tx.execute(
                "INSERT INTO oauth_connector_client_secret_versions(connector_id,version,secret_ref,status,created_at) VALUES(?1,?2,?3,'active',?4)",
                params![id,new_version,new_ref,at],
            ))?;
            db(tx.execute(
                "UPDATE oauth_connector_accounts SET client_secret_ref=?1,client_secret_version=?2,rotated_at=?3 WHERE connector_id=?4",
                params![new_ref,new_version,at,id],
            ))?;
            connector_audit(
                &tx,
                &user,
                "oauth.client_secret_rotated",
                id,
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
                "OAuth client-secret rotation did not commit",
            ));
        }
        let _ = secrets.delete(&old_ref);
        Ok(row(&c, id)?.info)
    }

    pub fn store_oauth_token_set(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        id: &str,
        token: &OAuthTokenSet,
    ) -> Result<OAuthConnectorInfo> {
        scopes(&token.scopes)?;
        let bytes = token_bytes(token)?;
        let (user, old_version, old_ref, store_id) = {
            let c = self.connection()?;
            let user = user_for_principal(&c, actor)?
                .ok_or(Error::Denied("OAuth connector owner unavailable"))?;
            let r = row(&c, id)?;
            if r.info.owner_user_id != user || r.info.status == "revoked" {
                return Err(Error::Denied("OAuth connector unavailable"));
            }
            if !token.scopes.is_subset(&r.info.requested_scopes) {
                return Err(Error::Denied(
                    "OAuth server granted scopes outside requested connector scope",
                ));
            }
            let sid: String = db(c.query_row(
                "SELECT value FROM store_meta WHERE key='store_id'",
                [],
                |x| x.get(0),
            ))?;
            (user, r.info.token_version, r.token_ref, sid)
        };
        let v = old_version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(Error::Limit("OAuth token version exhausted"))?;
        let tr = ref_for(&store_id, id, "token", v)?;
        secrets.put(&tr, &bytes)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = row(&tx, id)?;
        if current.info.owner_user_id != user
            || current.info.token_version != old_version
            || current.token_ref != old_ref
            || current.info.status == "revoked"
        {
            drop(tx);
            let _ = secrets.delete(&tr);
            return Err(Error::Conflict(
                "OAuth connector changed during token rotation",
            ));
        }
        let at = now();
        let result = (|| -> Result<()> {
            if let (Some(ov), Some(or)) = (old_version, old_ref.as_deref()) {
                db(tx.execute("UPDATE oauth_connector_token_versions SET status='superseded',retired_at=?1 WHERE connector_id=?2 AND version=?3 AND token_ref=?4 AND status IN ('active','unavailable')",params![at,id,ov,or]))?;
            }
            db(tx.execute("INSERT INTO oauth_connector_token_versions(connector_id,version,token_ref,status,granted_scopes_json,access_expires_at,created_at) VALUES(?1,?2,?3,'active',?4,?5,?6)",params![id,v,tr,json(&token.scopes)?,token.access_expires_at,at]))?;
            db(tx.execute("UPDATE oauth_connector_accounts SET token_ref=?1,token_version=?2,granted_scopes_json=?3,access_expires_at=?4,status='active',rotated_at=?5 WHERE connector_id=?6",params![tr,v,json(&token.scopes)?,token.access_expires_at,at,id]))?;
            connector_audit(
                &tx,
                &user,
                "oauth.token_set_rotated",
                id,
                serde_json::json!({"token_version":v,"granted_scopes":token.scopes,"access_expires_at":token.access_expires_at}),
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(tx);
            let _ = secrets.delete(&tr);
            return Err(error);
        }
        if db(tx.commit()).is_err() {
            let _ = secrets.delete(&tr);
            return Err(Error::Storage("OAuth token rotation did not commit"));
        }
        if let Some(or) = old_ref {
            let _ = secrets.delete(&or);
        }
        Ok(row(&c, id)?.info)
    }
    pub fn load_oauth_token_set(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        id: &str,
    ) -> Result<OAuthTokenSet> {
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?
            .ok_or(Error::Denied("OAuth connector owner unavailable"))?;
        let r = row(&c, id)?;
        if r.info.owner_user_id != user || r.info.status != "active" {
            return Err(Error::Denied("OAuth connector has no active token set"));
        }
        let (tr, v) = (
            r.token_ref
                .as_deref()
                .ok_or(Error::Denied("OAuth token ref unavailable"))?,
            r.info
                .token_version
                .ok_or(Error::Denied("OAuth token version unavailable"))?,
        );
        let active:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM oauth_connector_token_versions WHERE connector_id=?1 AND version=?2 AND token_ref=?3 AND status='active')",params![id,v,tr],|x|x.get(0)))?;
        if !active {
            return Err(Error::Denied("OAuth token version is not active"));
        }
        let raw = secrets.get(tr)?;
        let token: OAuthTokenSet = serde_json::from_slice(&raw)
            .map_err(|_| Error::Storage("invalid OAuth token set in SecretStore"))?;
        Ok(token)
    }
    pub fn revoke_oauth_connector(
        &self,
        actor: &PrincipalId,
        secrets: &dyn SecretStore,
        id: &str,
    ) -> Result<OAuthConnectorInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("OAuth connector owner unavailable"))?;
        let r = row(&tx, id)?;
        if r.info.owner_user_id != user {
            return Err(Error::Denied("OAuth connector belongs to another user"));
        }
        let refs = [r.client_secret_ref.clone(), r.token_ref.clone()];
        let at = now();
        db(tx.execute("UPDATE oauth_connector_accounts SET status='revoked',revoked_at=COALESCE(revoked_at,?1) WHERE connector_id=?2",params![at,id]))?;
        db(tx.execute("UPDATE oauth_connector_client_secret_versions SET status='revoked',retired_at=COALESCE(retired_at,?1) WHERE connector_id=?2",params![at,id]))?;
        db(tx.execute("UPDATE oauth_connector_token_versions SET status='revoked',retired_at=COALESCE(retired_at,?1) WHERE connector_id=?2",params![at,id]))?;
        connector_audit(
            &tx,
            &user,
            "oauth.connector_revoked",
            id,
            serde_json::json!({"client_secret_version":r.info.client_secret_version,"token_version":r.info.token_version}),
        )?;
        db(tx.commit())?;
        for r in refs.into_iter().flatten() {
            let _ = secrets.delete(&r);
        }
        Ok(row(&c, id)?.info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requested() -> BTreeSet<String> {
        BTreeSet::from(["read".into(), "write".into()])
    }

    #[test]
    fn oauth_token_versions_live_only_in_secretstore_and_rotate() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        store
            .create_oauth_connector(
                &actor,
                &secrets,
                "calendar",
                "https://auth.example.invalid/authorize",
                "https://auth.example.invalid/token",
                "client",
                "none",
                requested(),
                None,
            )
            .unwrap();
        let first = OAuthTokenSet {
            access_token: "access-v1".into(),
            refresh_token: Some("refresh-v1".into()),
            id_token: None,
            oidc_issuer: None,
            oidc_subject: None,
            token_type: "Bearer".into(),
            scopes: BTreeSet::from(["read".into()]),
            access_expires_at: None,
        };
        let info = store
            .store_oauth_token_set(&actor, &secrets, "calendar", &first)
            .unwrap();
        assert_eq!(info.token_version, Some(1));
        let loaded = store
            .load_oauth_token_set(&actor, &secrets, "calendar")
            .unwrap();
        assert_eq!(loaded.access_token, "access-v1");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-v1"));
        let second = OAuthTokenSet {
            access_token: "access-v2".into(),
            refresh_token: Some("refresh-v2".into()),
            id_token: None,
            oidc_issuer: None,
            oidc_subject: None,
            token_type: "Bearer".into(),
            scopes: BTreeSet::from(["read".into(), "write".into()]),
            access_expires_at: Some(123),
        };
        let info = store
            .store_oauth_token_set(&actor, &secrets, "calendar", &second)
            .unwrap();
        assert_eq!(info.token_version, Some(2));
        let c = store.connection().unwrap();
        let leaked: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM oauth_connector_accounts WHERE CAST(granted_scopes_json AS TEXT) LIKE '%access-v%' OR CAST(granted_scopes_json AS TEXT) LIKE '%refresh-v%')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!leaked);
        drop(c);
        store
            .revoke_oauth_connector(&actor, &secrets, "calendar")
            .unwrap();
        assert!(store
            .load_oauth_token_set(&actor, &secrets, "calendar")
            .is_err());
    }

    #[test]
    fn oauth_oidc_identity_is_exact_and_secret_store_only() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        store
            .create_oauth_connector(
                &actor,
                &secrets,
                "chatgpt-fixture",
                "https://auth.example.invalid/authorize",
                "https://auth.example.invalid/token",
                "client",
                "none",
                BTreeSet::from(["openid".into()]),
                None,
            )
            .unwrap();
        let token = OAuthTokenSet {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            id_token: Some("id-token".into()),
            oidc_issuer: Some("https://auth.example.invalid".into()),
            oidc_subject: Some("subject-123".into()),
            token_type: "Bearer".into(),
            scopes: BTreeSet::from(["openid".into()]),
            access_expires_at: None,
        };
        store
            .store_oauth_token_set(&actor, &secrets, "chatgpt-fixture", &token)
            .unwrap();
        let loaded = store
            .load_oauth_token_set(&actor, &secrets, "chatgpt-fixture")
            .unwrap();
        assert_eq!(
            loaded.oidc_issuer.as_deref(),
            Some("https://auth.example.invalid")
        );
        assert_eq!(loaded.oidc_subject.as_deref(), Some("subject-123"));

        let db = store.connection().unwrap();
        let leaked: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM oauth_connector_accounts WHERE CAST(requested_scopes_json AS TEXT) LIKE '%subject-123%' OR CAST(granted_scopes_json AS TEXT) LIKE '%subject-123%' OR client_id LIKE '%subject-123%')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!leaked);
        drop(db);

        let partial = OAuthTokenSet {
            access_token: "access".into(),
            refresh_token: None,
            id_token: None,
            oidc_issuer: Some("https://auth.example.invalid".into()),
            oidc_subject: None,
            token_type: "Bearer".into(),
            scopes: BTreeSet::from(["openid".into()]),
            access_expires_at: None,
        };
        assert!(store
            .store_oauth_token_set(&actor, &secrets, "chatgpt-fixture", &partial)
            .is_err());
    }

    #[test]
    fn oauth_scope_escalation_and_wrong_token_type_are_denied() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        store
            .create_oauth_connector(
                &actor,
                &secrets,
                "drive",
                "https://auth.example.invalid/authorize",
                "https://auth.example.invalid/token",
                "client",
                "none",
                BTreeSet::from(["read".into()]),
                None,
            )
            .unwrap();
        let escalated = OAuthTokenSet {
            access_token: "access".into(),
            refresh_token: None,
            id_token: None,
            oidc_issuer: None,
            oidc_subject: None,
            token_type: "Bearer".into(),
            scopes: BTreeSet::from(["read".into(), "admin".into()]),
            access_expires_at: None,
        };
        assert!(store
            .store_oauth_token_set(&actor, &secrets, "drive", &escalated)
            .is_err());
        let non_bearer = OAuthTokenSet {
            access_token: "access".into(),
            refresh_token: None,
            id_token: None,
            oidc_issuer: None,
            oidc_subject: None,
            token_type: "MAC".into(),
            scopes: BTreeSet::from(["read".into()]),
            access_expires_at: None,
        };
        assert!(store
            .store_oauth_token_set(&actor, &secrets, "drive", &non_bearer)
            .is_err());
    }

    #[test]
    fn oauth_client_secret_rotation_uses_versions() {
        let store = SessionStore::in_memory().unwrap();
        let secrets = MemorySecretStore::default();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let first = store
            .create_oauth_connector(
                &actor,
                &secrets,
                "mail",
                "https://auth.example.invalid/authorize",
                "https://auth.example.invalid/token",
                "client",
                "client_secret_post",
                BTreeSet::from(["read".into()]),
                Some(b"client-v1"),
            )
            .unwrap();
        assert_eq!(first.client_secret_version, Some(1));
        let second = store
            .rotate_oauth_connector_client_secret(&actor, &secrets, "mail", b"client-v2")
            .unwrap();
        assert_eq!(second.client_secret_version, Some(2));
        assert_eq!(
            store
                .resolve_oauth_client_secret(&actor, "mail", &secrets)
                .unwrap()
                .unwrap()
                .as_slice(),
            b"client-v2"
        );
    }
}
