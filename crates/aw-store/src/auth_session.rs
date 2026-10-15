use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const MIN_AUTH_TTL_SECONDS: u64 = 60;
const MAX_AUTH_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
const MAX_AUTH_CLIENT_JSON: usize = 4096;
const REFRESH_TOKEN_BYTES: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuthSessionInfo {
    pub session_id: String,
    pub user_id: String,
    pub credential_id: Option<String>,
    pub auth_method: String,
    pub rotation_family_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub rotated_to_session_id: Option<String>,
    pub rotated_at: Option<i64>,
    pub security_epoch_at_issue: u64,
    pub client: serde_json::Value,
}

#[derive(Debug)]
pub struct AuthSessionIssued {
    pub info: AuthSessionInfo,
    pub refresh_token: Zeroizing<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuthTokenFamilyInfo {
    pub family_id: String,
    pub user_id: String,
    pub credential_id: Option<String>,
    pub portable_binding_id: Option<String>,
    pub oidc_identity_id: Option<String>,
    pub auth_method: String,
    pub status: String,
    pub current_session_id: Option<String>,
    pub security_epoch_at_issue: u64,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_rotated_at: Option<i64>,
    pub reuse_detected_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub client: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedSession {
    pub session: AuthSessionInfo,
    pub stable_principal: String,
}

fn validate_auth_method(method: &str) -> Result<()> {
    if !matches!(
        method,
        "unix" | "device_key" | "passkey" | "password_verifier" | "oidc"
    ) {
        return Err(Error::Config(
            "unsupported native authentication method".into(),
        ));
    }
    Ok(())
}

fn validate_client(client: &serde_json::Value) -> Result<String> {
    if !client.is_object() {
        return Err(Error::Config(
            "auth-session client metadata must be a JSON object".into(),
        ));
    }
    let encoded = json(client)?;
    if encoded.len() > MAX_AUTH_CLIENT_JSON {
        return Err(Error::Limit(
            "auth-session client metadata exceeds 4096 bytes",
        ));
    }
    Ok(encoded)
}

fn expiry_from_ttl(ttl_seconds: u64, issued_at: i64) -> Result<i64> {
    if !(MIN_AUTH_TTL_SECONDS..=MAX_AUTH_TTL_SECONDS).contains(&ttl_seconds) {
        return Err(Error::Config(
            "auth-session TTL must be between 60 seconds and 30 days".into(),
        ));
    }
    let delta = ttl_seconds
        .checked_mul(1_000_000)
        .and_then(|v| i64::try_from(v).ok())
        .ok_or(Error::Limit("auth-session TTL overflow"))?;
    issued_at
        .checked_add(delta)
        .ok_or(Error::Limit("auth-session expiry overflow"))
}

fn new_refresh_token() -> Result<Zeroizing<String>> {
    let mut bytes = [0u8; REFRESH_TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| Error::Storage("OS randomness unavailable"))?;
    Ok(Zeroizing::new(format!(
        "awrt_{}",
        URL_SAFE_NO_PAD.encode(bytes)
    )))
}

fn refresh_hash(token: &str) -> Result<String> {
    if !token.starts_with("awrt_") || token.len() < 30 || token.len() > 128 {
        return Err(Error::Denied("invalid auth-session refresh token"));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"aw-auth-refresh-v1\0");
    hasher.update(token.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn auth_audit(
    c: &Connection,
    actor_user_id: &str,
    auth_session_id: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    metadata: serde_json::Value,
) -> Result<()> {
    let store: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |r| r.get(0),
    ))?;
    db(c.execute(
        "INSERT INTO security_audit_events(audit_id,store_id,realm_id,actor_user_id,auth_session_id,action,resource_type,resource_id,outcome,metadata_json,created_at) SELECT ?1,?2,u.realm_id,?3,?4,?5,?6,?7,'allowed',?8,?9 FROM users u WHERE u.user_id=?3",
        params![id("audit")?,store,actor_user_id,auth_session_id,action,resource_type,resource_id,json(&metadata)?,now()],
    ))?;
    Ok(())
}

fn session_row(c: &Connection, session_id: &str) -> Result<AuthSessionInfo> {
    let row: (String,String,Option<String>,String,String,i64,i64,Option<i64>,Option<i64>,u64,String,Option<String>,Option<i64>)=db(c.query_row(
        "SELECT session_id,user_id,credential_id,auth_method,rotation_family_id,issued_at,expires_at,last_seen_at,revoked_at,security_epoch_at_issue,client_json,rotated_to_session_id,rotated_at FROM auth_sessions WHERE session_id=?1",
        [session_id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?)),
    ).optional())?.ok_or(Error::Denied("auth session unavailable"))?;
    Ok(AuthSessionInfo {
        session_id: row.0,
        user_id: row.1,
        credential_id: row.2,
        auth_method: row.3,
        rotation_family_id: row.4,
        issued_at: row.5,
        expires_at: row.6,
        last_seen_at: row.7,
        revoked_at: row.8,
        security_epoch_at_issue: row.9,
        client: decode(&row.10)?,
        rotated_to_session_id: row.11,
        rotated_at: row.12,
    })
}

fn family_row(c: &Connection, family_id: &str) -> Result<AuthTokenFamilyInfo> {
    let row:(String,String,Option<String>,Option<String>,Option<String>,String,String,Option<String>,u64,i64,i64,Option<i64>,Option<i64>,Option<i64>,String)=db(c.query_row(
        "SELECT family_id,user_id,credential_id,portable_binding_id,oidc_identity_id,auth_method,status,current_session_id,security_epoch_at_issue,created_at,expires_at,last_rotated_at,reuse_detected_at,revoked_at,client_json FROM auth_token_families WHERE family_id=?1",
        [family_id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?,r.get(14)?)),
    ).optional())?.ok_or(Error::Denied("auth-session family unavailable"))?;
    Ok(AuthTokenFamilyInfo {
        family_id: row.0,
        user_id: row.1,
        credential_id: row.2,
        portable_binding_id: row.3,
        oidc_identity_id: row.4,
        auth_method: row.5,
        status: row.6,
        current_session_id: row.7,
        security_epoch_at_issue: row.8,
        created_at: row.9,
        expires_at: row.10,
        last_rotated_at: row.11,
        reuse_detected_at: row.12,
        revoked_at: row.13,
        client: decode(&row.14)?,
    })
}

fn validate_credential(
    c: &Connection,
    user_id: &str,
    auth_method: &str,
    credential_id: Option<&str>,
) -> Result<()> {
    let Some(credential_id) = credential_id else {
        if matches!(auth_method, "passkey" | "password_verifier") {
            return Err(Error::Denied(
                "this authentication method requires an explicit local credential",
            ));
        }
        return Ok(());
    };
    let kind: Option<String> = db(c
        .query_row(
            "SELECT kind FROM local_auth_credentials WHERE credential_id=?1 AND user_id=?2 AND revoked_at IS NULL",
            params![credential_id,user_id],
            |r| r.get(0),
        )
        .optional())?;
    let expected = match auth_method {
        "device_key" => "device_key",
        "passkey" => "passkey",
        "password_verifier" => "password_verifier",
        "unix" | "oidc" => {
            return Err(Error::Denied(
                "unix/OIDC auth sessions do not use local credential rows",
            ))
        }
        _ => {
            return Err(Error::Config(
                "unsupported native authentication method".into(),
            ))
        }
    };
    if kind.as_deref() != Some(expected) {
        return Err(Error::Denied(
            "authentication credential is unavailable or incompatible",
        ));
    }
    Ok(())
}

fn family_auth_binding_valid(c: &Connection, family: &AuthTokenFamilyInfo) -> Result<bool> {
    if let Some(binding_id) = family.portable_binding_id.as_deref() {
        let portable: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM portable_identity_bindings b JOIN users u ON u.user_id=b.user_id JOIN domains d ON d.domain_id=b.domain_id JOIN identity_continuity_grants g ON g.domain_id=b.domain_id AND g.source_node_id=b.source_node_id WHERE b.binding_id=?1 AND b.user_id=?2 AND b.kind='device_key' AND b.status='active' AND u.status='active' AND u.security_epoch=b.user_security_epoch AND u.security_epoch=?3 AND d.status='active' AND d.security_epoch=b.domain_security_epoch AND g.status='active')",
            params![binding_id,family.user_id,family.security_epoch_at_issue],
            |r| r.get(0),
        ))?;
        if !portable {
            return Ok(false);
        }
    }
    if let Some(identity_id) = family.oidc_identity_id.as_deref() {
        let oidc: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM user_identities i JOIN users u ON u.user_id=i.user_id JOIN oidc_providers p ON p.provider_id=i.provider_id WHERE i.identity_id=?1 AND i.user_id=?2 AND i.kind='oidc' AND i.status='active' AND i.login_verified_at IS NOT NULL AND p.status='active' AND u.status='active' AND u.security_epoch=?3)",
            params![identity_id,family.user_id,family.security_epoch_at_issue],
            |r| r.get(0),
        ))?;
        if !oidc {
            return Ok(false);
        }
    }
    Ok(true)
}

fn revoke_family_tx(
    c: &Connection,
    family: &AuthTokenFamilyInfo,
    reuse: bool,
    audit_session_id: Option<&str>,
) -> Result<()> {
    let at = now();
    db(c.execute(
        "UPDATE auth_token_families SET status='revoked',revoked_at=COALESCE(revoked_at,?1),reuse_detected_at=CASE WHEN ?2 THEN COALESCE(reuse_detected_at,?1) ELSE reuse_detected_at END WHERE family_id=?3",
        params![at,reuse,family.family_id],
    ))?;
    db(c.execute(
        "UPDATE auth_sessions SET revoked_at=COALESCE(revoked_at,?1) WHERE rotation_family_id=?2",
        params![at, family.family_id],
    ))?;
    auth_audit(
        c,
        &family.user_id,
        audit_session_id,
        if reuse {
            "auth.session_token_reuse_detected"
        } else {
            "auth.session_family_revoked"
        },
        "auth_session_family",
        Some(&family.family_id),
        serde_json::json!({"reuse_detected":reuse}),
    )?;
    Ok(())
}

impl SessionStore {
    pub fn issue_auth_session(
        &self,
        actor: &PrincipalId,
        auth_method: &str,
        credential_id: Option<&str>,
        ttl_seconds: u64,
        client: serde_json::Value,
    ) -> Result<AuthSessionIssued> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user_id = ensure_user_for_principal(&tx, actor)?;
        if auth_method != "unix" {
            return Err(Error::Denied(
                "generic native auth-session issue is limited to the already-authenticated local Unix principal; device/passkey/password sessions require their verified proof boundary",
            ));
        }
        if credential_id.is_some() {
            return Err(Error::Denied(
                "Unix auth sessions do not accept a credential row",
            ));
        }
        issue_for_user_tx(
            &tx,
            &user_id,
            auth_method,
            credential_id,
            None,
            None,
            ttl_seconds,
            client,
        )
        .and_then(|issued| {
            db(tx.commit())?;
            Ok(issued)
        })
    }

    /// Issue a native refresh family from a successfully verified Phase 30
    /// portable device proof. The family remains bound to that exact imported
    /// binding/source trust on every verify/rotation.
    pub fn issue_auth_session_from_portable_auth(
        &self,
        authenticated: &AuthenticatedStableUser,
        ttl_seconds: u64,
        client: serde_json::Value,
    ) -> Result<AuthSessionIssued> {
        if authenticated.method != "device_key"
            || authenticated.stable_principal
                != stable_user_principal(&authenticated.user_id)?.as_str()
        {
            return Err(Error::Denied(
                "portable authentication result is not a device-key proof",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let valid: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM portable_identity_bindings b JOIN users u ON u.user_id=b.user_id JOIN domains d ON d.domain_id=b.domain_id JOIN identity_continuity_grants g ON g.domain_id=b.domain_id AND g.source_node_id=b.source_node_id WHERE b.binding_id=?1 AND b.user_id=?2 AND b.domain_id=?3 AND b.kind='device_key' AND b.status='active' AND u.status='active' AND u.security_epoch=?4 AND u.security_epoch=b.user_security_epoch AND d.status='active' AND d.security_epoch=?5 AND d.security_epoch=b.domain_security_epoch AND g.status='active')",
            params![authenticated.binding_id,authenticated.user_id,authenticated.domain_id,authenticated.user_security_epoch,authenticated.domain_security_epoch],
            |r| r.get(0),
        ))?;
        if !valid {
            return Err(Error::Denied(
                "portable device proof is stale, revoked or no longer trusted",
            ));
        }
        let issued = issue_for_user_tx(
            &tx,
            &authenticated.user_id,
            "device_key",
            None,
            Some(&authenticated.binding_id),
            None,
            ttl_seconds,
            client,
        )?;
        db(tx.commit())?;
        Ok(issued)
    }

    pub fn issue_auth_session_from_verified_oidc(
        &self,
        identity_id: &str,
        ttl_seconds: u64,
        client: serde_json::Value,
    ) -> Result<AuthSessionIssued> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user_id: String = db(tx
            .query_row(
                "SELECT i.user_id FROM user_identities i JOIN users u ON u.user_id=i.user_id WHERE i.identity_id=?1 AND i.kind='oidc' AND i.status='active' AND i.login_verified_at IS NOT NULL AND u.status='active'",
                [identity_id],
                |r| r.get(0),
            )
            .optional())?
        .ok_or(Error::Denied("verified OIDC identity is unavailable"))?;
        let issued = issue_for_user_tx(
            &tx,
            &user_id,
            "oidc",
            None,
            None,
            Some(identity_id),
            ttl_seconds,
            client,
        )?;
        db(tx.commit())?;
        Ok(issued)
    }

    pub fn authenticate_auth_session(&self, refresh_token: &str) -> Result<AuthenticatedSession> {
        let hash = refresh_hash(refresh_token)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let session_id: Option<String> = db(tx
            .query_row(
                "SELECT session_id FROM auth_sessions WHERE refresh_token_hash=?1",
                [&hash],
                |r| r.get(0),
            )
            .optional())?;
        let Some(session_id) = session_id else {
            return Err(Error::Denied("auth-session refresh token is invalid"));
        };
        let session = session_row(&tx, &session_id)?;
        let family = family_row(&tx, &session.rotation_family_id)?;
        if !family_auth_binding_valid(&tx, &family)? {
            revoke_family_tx(&tx, &family, false, Some(&session.session_id))?;
            db(tx.commit())?;
            return Err(Error::Denied(
                "auth session lost its portable identity/source trust",
            ));
        }
        if session.rotated_to_session_id.is_some()
            || family.current_session_id.as_deref() != Some(session.session_id.as_str())
        {
            revoke_family_tx(&tx, &family, true, Some(&session.session_id))?;
            db(tx.commit())?;
            return Err(Error::Denied(
                "rotated auth-session token reuse revoked the token family",
            ));
        }
        let current = now();
        if family.status != "active"
            || session.revoked_at.is_some()
            || current >= session.expires_at
            || current >= family.expires_at
        {
            return Err(Error::Denied("auth session is expired or revoked"));
        }
        let user: Option<(String, u64)> = db(tx
            .query_row(
                "SELECT status,security_epoch FROM users WHERE user_id=?1",
                [&session.user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        let valid = user.is_some_and(|(status, epoch)| {
            status == "active"
                && epoch == session.security_epoch_at_issue
                && epoch == family.security_epoch_at_issue
        });
        if !valid {
            revoke_family_tx(&tx, &family, false, Some(&session.session_id))?;
            db(tx.commit())?;
            return Err(Error::Denied(
                "auth session invalidated by user security epoch",
            ));
        }
        db(tx.execute(
            "UPDATE auth_sessions SET last_seen_at=?1 WHERE session_id=?2",
            params![current, session.session_id],
        ))?;
        db(tx.commit())?;
        let mut info = session;
        info.last_seen_at = Some(current);
        Ok(AuthenticatedSession {
            stable_principal: stable_user_principal(&info.user_id)?.as_str().to_owned(),
            session: info,
        })
    }

    pub fn rotate_auth_session(&self, refresh_token: &str) -> Result<AuthSessionIssued> {
        let hash = refresh_hash(refresh_token)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let session_id: Option<String> = db(tx
            .query_row(
                "SELECT session_id FROM auth_sessions WHERE refresh_token_hash=?1",
                [&hash],
                |r| r.get(0),
            )
            .optional())?;
        let Some(session_id) = session_id else {
            return Err(Error::Denied("auth-session refresh token is invalid"));
        };
        let old = session_row(&tx, &session_id)?;
        let family = family_row(&tx, &old.rotation_family_id)?;
        if !family_auth_binding_valid(&tx, &family)? {
            revoke_family_tx(&tx, &family, false, Some(&old.session_id))?;
            db(tx.commit())?;
            return Err(Error::Denied(
                "auth session lost its portable identity/source trust",
            ));
        }
        if old.rotated_to_session_id.is_some()
            || family.current_session_id.as_deref() != Some(old.session_id.as_str())
        {
            revoke_family_tx(&tx, &family, true, Some(&old.session_id))?;
            db(tx.commit())?;
            return Err(Error::Denied(
                "rotated auth-session token reuse revoked the token family",
            ));
        }
        let current = now();
        if family.status != "active"
            || old.revoked_at.is_some()
            || current >= old.expires_at
            || current >= family.expires_at
        {
            return Err(Error::Denied("auth session is expired or revoked"));
        }
        let epoch: Option<u64> = db(tx
            .query_row(
                "SELECT security_epoch FROM users WHERE user_id=?1 AND status='active'",
                [&old.user_id],
                |r| r.get(0),
            )
            .optional())?;
        if epoch != Some(old.security_epoch_at_issue)
            || epoch != Some(family.security_epoch_at_issue)
        {
            revoke_family_tx(&tx, &family, false, Some(&old.session_id))?;
            db(tx.commit())?;
            return Err(Error::Denied(
                "auth session invalidated by user security epoch",
            ));
        }
        let token = new_refresh_token()?;
        let new_hash = refresh_hash(&token)?;
        let new_session = id("auths")?;
        db(tx.execute("INSERT INTO auth_sessions(session_id,user_id,credential_id,auth_method,refresh_token_hash,rotation_family_id,issued_at,expires_at,last_seen_at,revoked_at,security_epoch_at_issue,client_json,rotated_to_session_id,rotated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL,NULL,?9,?10,NULL,NULL)",params![new_session,old.user_id,old.credential_id,old.auth_method,new_hash,old.rotation_family_id,current,family.expires_at,old.security_epoch_at_issue,json(&old.client)?]))?;
        db(tx.execute("UPDATE auth_sessions SET revoked_at=?1,rotated_to_session_id=?2,rotated_at=?1 WHERE session_id=?3 AND revoked_at IS NULL AND rotated_to_session_id IS NULL",params![current,new_session,old.session_id]))?;
        db(tx.execute("UPDATE auth_token_families SET current_session_id=?1,last_rotated_at=?2 WHERE family_id=?3 AND status='active'",params![new_session,current,family.family_id]))?;
        auth_audit(
            &tx,
            &old.user_id,
            Some(&new_session),
            "auth.session_rotated",
            "auth_session_family",
            Some(&family.family_id),
            serde_json::json!({"from_session_id":old.session_id,"to_session_id":new_session}),
        )?;
        let info = session_row(&tx, &new_session)?;
        db(tx.commit())?;
        Ok(AuthSessionIssued {
            info,
            refresh_token: token,
        })
    }

    pub fn revoke_auth_session_by_token(&self, refresh_token: &str) -> Result<AuthTokenFamilyInfo> {
        let hash = refresh_hash(refresh_token)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let session_id: Option<String> = db(tx
            .query_row(
                "SELECT session_id FROM auth_sessions WHERE refresh_token_hash=?1",
                [&hash],
                |r| r.get(0),
            )
            .optional())?;
        let Some(session_id) = session_id else {
            return Err(Error::Denied("auth-session refresh token is invalid"));
        };
        let session = session_row(&tx, &session_id)?;
        let family = family_row(&tx, &session.rotation_family_id)?;
        if family.status == "active" {
            revoke_family_tx(&tx, &family, false, Some(&session.session_id))?;
        }
        db(tx.commit())?;
        family_row(&c, &family.family_id)
    }

    pub fn revoke_auth_session_family(
        &self,
        actor: &PrincipalId,
        family_id: &str,
    ) -> Result<AuthTokenFamilyInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let actor_user = user_for_principal(&tx, actor)?.ok_or(Error::Denied(
            "auth-session revocation requires an active known user",
        ))?;
        let family = family_row(&tx, family_id)?;
        if family.user_id != actor_user {
            return Err(Error::Denied("auth-session family belongs to another user"));
        }
        if family.status == "active" {
            revoke_family_tx(&tx, &family, false, family.current_session_id.as_deref())?;
        }
        db(tx.commit())?;
        family_row(&c, family_id)
    }

    pub fn auth_sessions(&self, actor: &PrincipalId) -> Result<Vec<AuthSessionInfo>> {
        let c = self.connection()?;
        let user = user_for_principal(&c, actor)?
            .ok_or(Error::Denied("auth sessions unavailable or not permitted"))?;
        let mut q=db(c.prepare("SELECT session_id FROM auth_sessions WHERE user_id=?1 ORDER BY issued_at DESC,session_id LIMIT 256"))?;
        let ids = db(db(q.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| session_row(&c, id)).collect()
    }
}

fn issue_for_user_tx(
    c: &Connection,
    user_id: &str,
    auth_method: &str,
    credential_id: Option<&str>,
    portable_binding_id: Option<&str>,
    oidc_identity_id: Option<&str>,
    ttl_seconds: u64,
    client: serde_json::Value,
) -> Result<AuthSessionIssued> {
    validate_auth_method(auth_method)?;
    let client_json = validate_client(&client)?;
    validate_credential(c, user_id, auth_method, credential_id)?;
    let epoch: u64 = db(c
        .query_row(
            "SELECT security_epoch FROM users WHERE user_id=?1 AND status='active'",
            [user_id],
            |r| r.get(0),
        )
        .optional())?
    .ok_or(Error::Denied("auth-session user unavailable"))?;
    let issued_at = now();
    let expires_at = expiry_from_ttl(ttl_seconds, issued_at)?;
    let family = id("authfam")?;
    let session = id("auths")?;
    let token = new_refresh_token()?;
    let hash = refresh_hash(&token)?;
    db(c.execute("INSERT INTO auth_token_families(family_id,user_id,credential_id,portable_binding_id,oidc_identity_id,auth_method,status,current_session_id,security_epoch_at_issue,created_at,expires_at,client_json) VALUES(?1,?2,?3,?4,?5,?6,'active',?7,?8,?9,?10,?11)",params![family,user_id,credential_id,portable_binding_id,oidc_identity_id,auth_method,session,epoch,issued_at,expires_at,client_json]))?;
    db(c.execute("INSERT INTO auth_sessions(session_id,user_id,credential_id,auth_method,refresh_token_hash,rotation_family_id,issued_at,expires_at,last_seen_at,revoked_at,security_epoch_at_issue,client_json,rotated_to_session_id,rotated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL,NULL,?9,?10,NULL,NULL)",params![session,user_id,credential_id,auth_method,hash,family,issued_at,expires_at,epoch,client_json]))?;
    if let Some(credential) = credential_id {
        db(c.execute(
            "UPDATE local_auth_credentials SET last_used_at=?1 WHERE credential_id=?2",
            params![issued_at, credential],
        ))?;
    }
    auth_audit(
        c,
        user_id,
        Some(&session),
        "auth.session_issued",
        "auth_session_family",
        Some(&family),
        serde_json::json!({"auth_method":auth_method,"expires_at":expires_at,"credential_id":credential_id}),
    )?;
    Ok(AuthSessionIssued {
        info: session_row(c, &session)?,
        refresh_token: token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_reuse_revokes_whole_family() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let issued = store
            .issue_auth_session(
                &actor,
                "unix",
                None,
                600,
                serde_json::json!({"client":"test"}),
            )
            .unwrap();
        assert_eq!(
            store
                .authenticate_auth_session(&issued.refresh_token)
                .unwrap()
                .session
                .session_id,
            issued.info.session_id
        );
        let rotated = store.rotate_auth_session(&issued.refresh_token).unwrap();
        assert!(store
            .authenticate_auth_session(&issued.refresh_token)
            .is_err());
        assert!(store
            .authenticate_auth_session(&rotated.refresh_token)
            .is_err());
        let sessions = store.auth_sessions(&actor).unwrap();
        assert!(sessions.iter().all(|session| session.revoked_at.is_some()));
    }

    #[test]
    fn security_epoch_invalidates_family() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let issued = store
            .issue_auth_session(&actor, "unix", None, 600, serde_json::json!({}))
            .unwrap();
        let user = store.current_user(&actor).unwrap();
        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE users SET security_epoch=security_epoch+1 WHERE user_id=?1",
                [user.user_id],
            )
            .unwrap();
        }
        assert!(store
            .authenticate_auth_session(&issued.refresh_token)
            .is_err());
        assert!(store.rotate_auth_session(&issued.refresh_token).is_err());
    }

    #[test]
    fn session_metadata_never_contains_refresh_token() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let issued = store
            .issue_auth_session(&actor, "unix", None, 600, serde_json::json!({}))
            .unwrap();
        let encoded = serde_json::to_string(&issued.info).unwrap();
        assert!(!encoded.contains(issued.refresh_token.as_str()));
        let c = store.connection().unwrap();
        let db_value: String = c
            .query_row(
                "SELECT refresh_token_hash FROM auth_sessions WHERE session_id=?1",
                [issued.info.session_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(db_value, issued.refresh_token.as_str());
        assert_eq!(db_value.len(), 64);
    }
}
