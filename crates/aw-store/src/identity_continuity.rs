use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const IDENTITY_CONTINUITY_VERSION: u16 = 2;
const MAX_CONTINUITY_USERS: usize = 512;
const MAX_BINDINGS_PER_USER: usize = 32;
const MAX_CONTINUITY_BYTES: usize = 1024 * 1024;

type BindingProjectionKey = (String, String, String, u64, u64, String);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StableUserInfo {
    pub user_id: String,
    pub realm_id: String,
    pub status: String,
    pub display: serde_json::Value,
    pub security_epoch: u64,
    pub created_at: i64,
    pub stable_principal: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserDeviceKeyInfo {
    pub device_id: String,
    pub user_id: String,
    pub public_key: String,
    pub status: String,
    pub security_epoch: u64,
    pub registered_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcIdentityInfo {
    pub identity_id: String,
    pub user_id: String,
    pub provider_id: Option<String>,
    pub issuer: String,
    pub subject: String,
    pub verified_at: Option<i64>,
    pub login_verified_at: Option<i64>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableIdentityDescriptor {
    pub kind: String,
    pub binding_key: String,
    pub device_id: Option<String>,
    pub public_key: Option<String>,
    pub provider_id: Option<String>,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub verified_at: Option<i64>,
    pub binding_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityContinuityUser {
    pub user: StableUserInfo,
    pub bindings: Vec<PortableIdentityDescriptor>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityContinuitySnapshot {
    pub version: u16,
    pub source_store_id: String,
    pub source_node_id: String,
    pub domain_id: String,
    pub domain_security_epoch: u64,
    pub users: Vec<IdentityContinuityUser>,
    pub snapshot_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityContinuityGrantInfo {
    pub id: String,
    pub domain_id: String,
    pub source_node_id: String,
    pub status: String,
    pub accepted_by_user_id: String,
    pub revision: u64,
    pub accepted_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableIdentityBindingInfo {
    pub id: String,
    pub domain_id: String,
    pub user_id: String,
    pub source_store_id: String,
    pub source_node_id: String,
    pub kind: String,
    pub binding_key: String,
    pub device_id: Option<String>,
    pub public_key: Option<String>,
    pub provider_id: Option<String>,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub user_security_epoch: u64,
    pub domain_security_epoch: u64,
    pub verified_at: Option<i64>,
    pub binding_sha256: String,
    pub status: String,
    pub imported_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityContinuityApplyResult {
    pub snapshot_id: String,
    pub domain_id: String,
    pub users: usize,
    pub active_bindings: usize,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedStableUser {
    pub user_id: String,
    pub stable_principal: String,
    pub domain_id: String,
    pub domain_role: Option<String>,
    pub user_security_epoch: u64,
    pub domain_security_epoch: u64,
    pub method: String,
    pub binding_id: String,
}

fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(json(value)?.as_bytes())))
}

fn decode_public_key(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Config(
            "identity device public key must be 32-byte lowercase/uppercase hex".into(),
        ));
    }
    let mut out = [0u8; 32];
    let bytes = value.as_bytes();
    for (index, slot) in out.iter_mut().enumerate() {
        let start = index * 2;
        let pair = std::str::from_utf8(&bytes[start..start + 2])
            .map_err(|_| Error::Config("invalid identity device public key".into()))?;
        *slot = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::Config("invalid identity device public key".into()))?;
    }
    Ok(out)
}

fn canonical_public_key(value: &str) -> Result<String> {
    Ok(decode_public_key(value)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn device_binding_key(public_key: &str) -> Result<String> {
    let public = decode_public_key(public_key)?;
    Ok(format!("device:{:x}", Sha256::digest(public)))
}

pub(crate) fn oidc_binding_key(issuer: &str, subject: &str) -> Result<String> {
    validate_oidc_component(issuer, 512, "issuer")?;
    validate_oidc_component(subject, 512, "subject")?;
    Ok(format!(
        "oidc:{:x}",
        Sha256::digest(format!("{issuer}\0{subject}").as_bytes())
    ))
}

pub(crate) fn validate_oidc_component(value: &str, max: usize, name: &'static str) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::Config(format!("invalid OIDC {name}")));
    }
    Ok(())
}

pub(crate) fn user_row(c: &Connection, user_id: &str) -> Result<StableUserInfo> {
    let row: (String, String, String, String, u64, i64) = db(c
        .query_row(
            "SELECT user_id,realm_id,status,display_json,security_epoch,created_at FROM users WHERE user_id=?1",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional())?
        .ok_or(Error::Denied("user unavailable or not permitted"))?;
    Ok(StableUserInfo {
        user_id: row.0.clone(),
        realm_id: row.1,
        status: row.2,
        display: decode(&row.3)?,
        security_epoch: row.4,
        created_at: row.5,
        stable_principal: stable_user_principal(&row.0)?.as_str().to_owned(),
    })
}

fn device_row(c: &Connection, device_id: &str) -> Result<UserDeviceKeyInfo> {
    let row: (String, String, String, String, u64, i64, Option<i64>) = db(c
        .query_row(
            "SELECT device_id,user_id,public_key,status,security_epoch,registered_at,revoked_at FROM user_devices WHERE device_id=?1 AND public_key IS NOT NULL",
            [device_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get::<_,Option<String>>(2)?.unwrap_or_default(),r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)),
        )
        .optional())?
        .ok_or(Error::Denied("identity device unavailable"))?;
    Ok(UserDeviceKeyInfo {
        device_id: row.0,
        user_id: row.1,
        public_key: row.2,
        status: row.3,
        security_epoch: row.4,
        registered_at: row.5,
        revoked_at: row.6,
    })
}

pub(crate) fn oidc_row(c: &Connection, identity_id: &str) -> Result<OidcIdentityInfo> {
    let row: (String, String, Option<String>, String, String, Option<i64>, Option<i64>, String) = db(c
        .query_row(
            "SELECT identity_id,user_id,provider_id,issuer,subject,verified_at,login_verified_at,status FROM user_identities WHERE identity_id=?1 AND kind='oidc'",
            [identity_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,Option<String>>(3)?.unwrap_or_default(),r.get::<_,Option<String>>(4)?.unwrap_or_default(),r.get(5)?,r.get(6)?,r.get(7)?)),
        )
        .optional())?
        .ok_or(Error::Denied("OIDC identity unavailable"))?;
    Ok(OidcIdentityInfo {
        identity_id: row.0,
        user_id: row.1,
        provider_id: row.2,
        issuer: row.3,
        subject: row.4,
        verified_at: row.5,
        login_verified_at: row.6,
        status: row.7,
    })
}

fn grant_row(c: &Connection, grant_id: &str) -> Result<IdentityContinuityGrantInfo> {
    db(c
        .query_row(
            "SELECT grant_id,domain_id,source_node_id,status,accepted_by_user_id,revision,accepted_at,revoked_at FROM identity_continuity_grants WHERE grant_id=?1",
            [grant_id],
            |r| Ok(IdentityContinuityGrantInfo{id:r.get(0)?,domain_id:r.get(1)?,source_node_id:r.get(2)?,status:r.get(3)?,accepted_by_user_id:r.get(4)?,revision:r.get(5)?,accepted_at:r.get(6)?,revoked_at:r.get(7)?}),
        )
        .optional())?
        .ok_or(Error::Denied("identity continuity grant unavailable"))
}

fn binding_row(c: &Connection, binding_id: &str) -> Result<PortableIdentityBindingInfo> {
    db(c
        .query_row(
            "SELECT binding_id,domain_id,user_id,source_store_id,source_node_id,kind,binding_key,device_id,public_key,provider_id,issuer,subject,user_security_epoch,domain_security_epoch,verified_at,binding_sha256,status,imported_at FROM portable_identity_bindings WHERE binding_id=?1",
            [binding_id],
            |r| Ok(PortableIdentityBindingInfo{id:r.get(0)?,domain_id:r.get(1)?,user_id:r.get(2)?,source_store_id:r.get(3)?,source_node_id:r.get(4)?,kind:r.get(5)?,binding_key:r.get(6)?,device_id:r.get(7)?,public_key:r.get(8)?,provider_id:r.get(9)?,issuer:r.get(10)?,subject:r.get(11)?,user_security_epoch:r.get(12)?,domain_security_epoch:r.get(13)?,verified_at:r.get(14)?,binding_sha256:r.get(15)?,status:r.get(16)?,imported_at:r.get(17)?}),
        )
        .optional())?
        .ok_or(Error::Denied("portable identity binding unavailable"))
}

fn snapshot_hash(snapshot: &IdentityContinuitySnapshot) -> Result<String> {
    let mut value = serde_json::to_value(snapshot)
        .map_err(|_| Error::Storage("cannot encode identity continuity snapshot"))?;
    value
        .as_object_mut()
        .ok_or(Error::Storage("invalid identity continuity snapshot shape"))?
        .insert(
            "snapshot_sha256".into(),
            serde_json::Value::String(String::new()),
        );
    digest(&value)
}

pub fn identity_continuity_snapshot_transcript(snapshot: &IdentityContinuitySnapshot) -> Vec<u8> {
    let fields: Vec<Vec<u8>> = vec![
        snapshot.source_store_id.as_bytes().to_vec(),
        snapshot.source_node_id.as_bytes().to_vec(),
        snapshot.domain_id.as_bytes().to_vec(),
        snapshot.domain_security_epoch.to_be_bytes().to_vec(),
        snapshot.snapshot_sha256.as_bytes().to_vec(),
    ];
    let mut out = Vec::with_capacity(384);
    out.extend_from_slice(b"aw-identity-continuity-v2\0");
    for field in fields {
        let len = u32::try_from(field.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&field);
    }
    out
}

fn verify_snapshot_signature(
    node_id: &str,
    public_key: &str,
    snapshot: &IdentityContinuitySnapshot,
    signature: &str,
) -> Result<()> {
    let public = decode_public_key(public_key)?;
    if format!("node_{:x}", Sha256::digest(public)) != node_id {
        return Err(Error::Protocol(
            "identity continuity source node does not match its public key",
        ));
    }
    let verifying = VerifyingKey::from_bytes(&public)
        .map_err(|_| Error::Protocol("invalid continuity Ed25519 public key"))?;
    let bytes = STANDARD
        .decode(signature)
        .map_err(|_| Error::Protocol("invalid identity continuity signature"))?;
    let bytes: [u8; 64] = bytes
        .try_into()
        .map_err(|_| Error::Protocol("identity continuity signature must be 64 bytes"))?;
    verifying
        .verify_strict(
            &identity_continuity_snapshot_transcript(snapshot),
            &Signature::from_bytes(&bytes),
        )
        .map_err(|_| Error::Denied("identity continuity signature verification failed"))
}

pub(crate) fn security_audit(
    c: &Connection,
    actor_user_id: &str,
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
        "INSERT INTO security_audit_events(audit_id,store_id,realm_id,actor_user_id,action,resource_type,resource_id,outcome,metadata_json,created_at) SELECT ?1,?2,u.realm_id,?3,?4,?5,?6,'allowed',?7,?8 FROM users u WHERE u.user_id=?3",
        params![id("audit")?,store,actor_user_id,action,resource_type,resource_id,json(&metadata)?,now()],
    ))?;
    Ok(())
}

fn require_domain_admin(c: &Connection, actor: &PrincipalId, domain_id: &str) -> Result<String> {
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "identity continuity export requires an active known Domain user",
    ))?;
    let role: Option<String> = db(c
        .query_row(
            "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
            params![domain_id,user],
            |r| r.get(0),
        )
        .optional())?;
    if !matches!(role.as_deref(), Some("owner" | "admin")) {
        return Err(Error::Denied(
            "identity continuity export requires Domain owner/admin authority",
        ));
    }
    Ok(user)
}

fn user_is_domain_admin(c: &Connection, user_id: &str, domain_id: &str) -> Result<bool> {
    db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active' AND role IN ('owner','admin'))",
        params![domain_id,user_id],
        |r| r.get(0),
    ))
}

fn can_accept_source(
    c: &Connection,
    actor: &PrincipalId,
    domain_id: &str,
    source_node_id: &str,
) -> Result<String> {
    let user = ensure_user_for_principal(c, actor)?;
    let admin = user_is_domain_admin(c, &user, domain_id)?;
    let replica_control: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspace_replica_origins o JOIN workspaces w ON w.workspace_id=o.workspace_id WHERE w.domain_id=?1 AND o.control_node_id=?2)",
        params![domain_id,source_node_id],
        |r| r.get(0),
    ))?;
    let authority_grant: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM domain_authority_peer_grants WHERE authority_domain_id=?1 AND accepted_from_node_id=?2 AND status='active')",
        params![domain_id,source_node_id],
        |r| r.get(0),
    ))?;
    if !admin && !replica_control && !authority_grant {
        return Err(Error::Denied(
            "identity continuity source must be explicitly related to this Domain authority/replica",
        ));
    }
    Ok(user)
}

fn portable_binding_id(domain_id: &str, kind: &str, binding_key: &str) -> Result<String> {
    let hash = digest(&serde_json::json!({
        "version": 1,
        "domain_id": domain_id,
        "kind": kind,
        "binding_key": binding_key,
    }))?;
    Ok(format!("pident_{}", &hash[..32]))
}

fn binding_descriptor_hash(
    kind: &str,
    binding_key: &str,
    device_id: Option<&str>,
    public_key: Option<&str>,
    provider_id: Option<&str>,
    issuer: Option<&str>,
    subject: Option<&str>,
    verified_at: Option<i64>,
) -> Result<String> {
    digest(&serde_json::json!({
        "version":1,
        "kind":kind,
        "binding_key":binding_key,
        "device_id":device_id,
        "public_key":public_key,
        "provider_id":provider_id,
        "issuer":issuer,
        "subject":subject,
        "verified_at":verified_at
    }))
}

fn validate_snapshot_users(
    snapshot: &IdentityContinuitySnapshot,
) -> Result<BTreeSet<BindingProjectionKey>> {
    let mut users = BTreeSet::new();
    let mut binding_keys = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    for entry in &snapshot.users {
        if entry.user.stable_principal != stable_user_principal(&entry.user.user_id)?.as_str() {
            return Err(Error::Conflict(
                "identity continuity stable principal does not match stable user ID",
            ));
        }
        if !users.insert(entry.user.user_id.clone()) {
            return Err(Error::Conflict(
                "duplicate stable user in identity continuity snapshot",
            ));
        }
        if entry.bindings.len() > MAX_BINDINGS_PER_USER {
            return Err(Error::Limit(
                "identity continuity bindings per user exceed 32",
            ));
        }
        for binding in &entry.bindings {
            validate_descriptor(binding)?;
            let expected = binding_descriptor_hash(
                &binding.kind,
                &binding.binding_key,
                binding.device_id.as_deref(),
                binding.public_key.as_deref(),
                binding.provider_id.as_deref(),
                binding.issuer.as_deref(),
                binding.subject.as_deref(),
                binding.verified_at,
            )?;
            if expected != binding.binding_sha256 {
                return Err(Error::Conflict("portable identity binding digest mismatch"));
            }
            if !binding_keys.insert((binding.kind.clone(), binding.binding_key.clone())) {
                return Err(Error::Conflict(
                    "duplicate portable identity binding in identity continuity snapshot",
                ));
            }
            bindings.insert((
                entry.user.user_id.clone(),
                binding.kind.clone(),
                binding.binding_key.clone(),
                entry.user.security_epoch,
                snapshot.domain_security_epoch,
                binding.binding_sha256.clone(),
            ));
        }
    }
    Ok(bindings)
}

fn projection_matches_snapshot(
    c: &Connection,
    snapshot: &IdentityContinuitySnapshot,
    source_node_id: &str,
    expected_bindings: &BTreeSet<BindingProjectionKey>,
) -> Result<bool> {
    for entry in &snapshot.users {
        let row: Option<(String, String, String, u64, i64)> = db(c
            .query_row(
                "SELECT realm_id,status,display_json,security_epoch,created_at FROM users WHERE user_id=?1",
                [&entry.user.user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional())?;
        let Some((realm, status, display, epoch, created_at)) = row else {
            return Ok(false);
        };
        if realm != entry.user.realm_id
            || status != entry.user.status
            || display != json(&entry.user.display)?
            || epoch != entry.user.security_epoch
            || created_at != entry.user.created_at
        {
            return Ok(false);
        }
    }

    let mut q=db(c.prepare("SELECT user_id,kind,binding_key,user_security_epoch,domain_security_epoch,binding_sha256 FROM portable_identity_bindings WHERE domain_id=?1 AND source_node_id=?2 AND status='active' ORDER BY user_id,kind,binding_key"))?;
    let rows = db(
        q.query_map(params![snapshot.domain_id, source_node_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u64>(3)?,
                r.get::<_, u64>(4)?,
                r.get::<_, String>(5)?,
            ))
        }),
    )?;
    let actual = db(rows.collect::<rusqlite::Result<BTreeSet<_>>>())?;
    Ok(&actual == expected_bindings)
}

impl SessionStore {
    pub fn current_user(&self, actor: &PrincipalId) -> Result<StableUserInfo> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        user_row(&c, &user)
    }

    pub fn register_identity_device_key(
        &self,
        actor: &PrincipalId,
        public_key: &str,
    ) -> Result<UserDeviceKeyInfo> {
        let public_key = canonical_public_key(public_key)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        if let Some((existing_id, existing_user, status)) = db(tx
            .query_row(
                "SELECT device_id,user_id,status FROM user_devices WHERE public_key=?1 ORDER BY registered_at LIMIT 1",
                [&public_key],
                |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)),
            )
            .optional())?
        {
            if existing_user != user {
                return Err(Error::Conflict(
                    "identity device public key is already linked to another stable aw user",
                ));
            }
            if status == "active" {
                drop(tx);
                return device_row(&c, &existing_id);
            }
            return Err(Error::Conflict(
                "revoked identity device key cannot be silently reactivated",
            ));
        }
        let key_hash = device_binding_key(&public_key)?;
        let device_id = format!("udev_{}", &key_hash[7..39]);
        let current_epoch: u64 = db(tx.query_row(
            "SELECT security_epoch FROM users WHERE user_id=?1 AND status='active'",
            [&user],
            |r| r.get(0),
        ))?;
        let next_epoch = current_epoch
            .checked_add(1)
            .ok_or(Error::Limit("user security epoch exhausted"))?;
        db(tx.execute(
            "UPDATE users SET security_epoch=?1 WHERE user_id=?2",
            params![next_epoch, user],
        ))?;
        db(tx.execute(
            "INSERT INTO user_devices(device_id,user_id,device_kind,public_key,status,capabilities_json,security_epoch,registered_at) VALUES(?1,?2,'native',?3,'active',?4,?5,?6)",
            params![device_id,user,public_key,json(&serde_json::json!({"identity_auth":true}))?,next_epoch,now()],
        ))?;
        security_audit(
            &tx,
            &user,
            "identity.device_linked",
            "user_device",
            Some(&device_id),
            serde_json::json!({"public_key_sha256":key_hash,"security_epoch":next_epoch}),
        )?;
        db(tx.commit())?;
        device_row(&c, &device_id)
    }

    pub fn revoke_identity_device_key(
        &self,
        actor: &PrincipalId,
        device_id: &str,
    ) -> Result<UserDeviceKeyInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = user_for_principal(&tx, actor)?.ok_or(Error::Denied(
            "identity device revocation requires an active known user",
        ))?;
        let device = device_row(&tx, device_id)?;
        if device.user_id != user {
            return Err(Error::Denied(
                "identity device belongs to another stable aw user",
            ));
        }
        if device.status == "active" {
            let epoch: u64 = db(tx.query_row(
                "SELECT security_epoch FROM users WHERE user_id=?1",
                [&user],
                |r| r.get(0),
            ))?;
            let next = epoch
                .checked_add(1)
                .ok_or(Error::Limit("user security epoch exhausted"))?;
            db(tx.execute(
                "UPDATE users SET security_epoch=?1 WHERE user_id=?2",
                params![next, user],
            ))?;
            db(tx.execute("UPDATE user_devices SET status='revoked',revoked_at=?1,security_epoch=security_epoch+1 WHERE device_id=?2 AND status='active'",params![now(),device_id]))?;
            security_audit(
                &tx,
                &user,
                "identity.device_revoked",
                "user_device",
                Some(device_id),
                serde_json::json!({"security_epoch":next}),
            )?;
        }
        db(tx.commit())?;
        device_row(&c, device_id)
    }

    pub fn link_oidc_identity(
        &self,
        actor: &PrincipalId,
        provider_id: Option<&str>,
        issuer: &str,
        subject: &str,
    ) -> Result<OidcIdentityInfo> {
        validate_oidc_component(issuer, 512, "issuer")?;
        validate_oidc_component(subject, 512, "subject")?;
        if let Some(provider) = provider_id {
            label(provider, 96)?;
        }
        let binding_key = oidc_binding_key(issuer, subject)?;
        let external_key = binding_key.clone();
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        if let Some((identity_id, existing_user, status)) = db(tx
            .query_row(
                "SELECT identity_id,user_id,status FROM user_identities WHERE external_key=?1",
                [&external_key],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional())?
        {
            if existing_user != user {
                return Err(Error::Conflict(
                    "OIDC issuer+subject is already linked to another stable aw user",
                ));
            }
            if status == "active" {
                drop(tx);
                return oidc_row(&c, &identity_id);
            }
            return Err(Error::Conflict(
                "revoked OIDC identity cannot be silently reactivated",
            ));
        }
        let identity_id = id("ident")?;
        let epoch: u64 = db(tx.query_row(
            "SELECT security_epoch FROM users WHERE user_id=?1",
            [&user],
            |r| r.get(0),
        ))?;
        let next = epoch
            .checked_add(1)
            .ok_or(Error::Limit("user security epoch exhausted"))?;
        db(tx.execute(
            "UPDATE users SET security_epoch=?1 WHERE user_id=?2",
            params![next, user],
        ))?;
        db(tx.execute("INSERT INTO user_identities(identity_id,user_id,kind,provider_id,issuer,subject,external_key,verified_at,status) VALUES(?1,?2,'oidc',?3,?4,?5,?6,?7,'active')",params![identity_id,user,provider_id,issuer,subject,external_key,now()]))?;
        security_audit(
            &tx,
            &user,
            "identity.oidc_linked",
            "user_identity",
            Some(&identity_id),
            serde_json::json!({"provider_id":provider_id,"issuer":issuer,"subject_sha256":format!("{:x}",Sha256::digest(subject.as_bytes())),"security_epoch":next}),
        )?;
        db(tx.commit())?;
        oidc_row(&c, &identity_id)
    }

    pub fn revoke_oidc_identity(
        &self,
        actor: &PrincipalId,
        identity_id: &str,
    ) -> Result<OidcIdentityInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = user_for_principal(&tx, actor)?.ok_or(Error::Denied(
            "OIDC identity revocation requires an active known user",
        ))?;
        let identity = oidc_row(&tx, identity_id)?;
        if identity.user_id != user {
            return Err(Error::Denied(
                "OIDC identity belongs to another stable aw user",
            ));
        }
        if identity.status == "active" {
            let epoch: u64 = db(tx.query_row(
                "SELECT security_epoch FROM users WHERE user_id=?1",
                [&user],
                |r| r.get(0),
            ))?;
            let next = epoch
                .checked_add(1)
                .ok_or(Error::Limit("user security epoch exhausted"))?;
            db(tx.execute(
                "UPDATE users SET security_epoch=?1 WHERE user_id=?2",
                params![next, user],
            ))?;
            db(tx.execute(
                "UPDATE user_identities SET status='revoked' WHERE identity_id=?1 AND kind='oidc' AND status='active'",
                [identity_id],
            ))?;
            security_audit(
                &tx,
                &user,
                "identity.oidc_revoked",
                "user_identity",
                Some(identity_id),
                serde_json::json!({
                    "issuer":identity.issuer,
                    "subject_sha256":format!("{:x}",Sha256::digest(identity.subject.as_bytes())),
                    "security_epoch":next
                }),
            )?;
        }
        db(tx.commit())?;
        oidc_row(&c, identity_id)
    }

    pub fn identity_continuity_accept(
        &self,
        actor: &PrincipalId,
        domain_id: &str,
        source_node_id: &str,
    ) -> Result<IdentityContinuityGrantInfo> {
        let peer = self.awnet_peer_by_node_id(source_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied(
                "identity continuity source peer is not trusted",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = can_accept_source(&tx, actor, domain_id, source_node_id)?;
        let existing:Option<String>=db(tx.query_row("SELECT grant_id FROM identity_continuity_grants WHERE domain_id=?1 AND source_node_id=?2",params![domain_id,source_node_id],|r|r.get(0)).optional())?;
        if let Some(id) = existing {
            let grant = grant_row(&tx, &id)?;
            if grant.status == "active" {
                return Ok(grant);
            }
            // Calling `continuity-accept` again is itself the explicit fresh trust
            // decision. Pairing or metadata import never reactivates this grant.
            db(tx.execute(
                "UPDATE identity_continuity_grants SET status='active',accepted_by_user_id=?1,revision=revision+1,accepted_at=?2,revoked_at=NULL WHERE grant_id=?3 AND status='revoked'",
                params![user,now(),id],
            ))?;
            security_audit(
                &tx,
                &user,
                "identity.continuity_source_reaccepted",
                "domain",
                Some(domain_id),
                serde_json::json!({"source_node_id":source_node_id,"grant_id":id}),
            )?;
            db(tx.commit())?;
            return grant_row(&c, &id);
        }
        let grant_id = id("icgrant")?;
        db(tx.execute("INSERT INTO identity_continuity_grants(grant_id,domain_id,source_node_id,status,accepted_by_user_id,accepted_at) VALUES(?1,?2,?3,'active',?4,?5)",params![grant_id,domain_id,source_node_id,user,now()]))?;
        security_audit(
            &tx,
            &user,
            "identity.continuity_source_accepted",
            "domain",
            Some(domain_id),
            serde_json::json!({"source_node_id":source_node_id,"grant_id":grant_id}),
        )?;
        db(tx.commit())?;
        grant_row(&c, &grant_id)
    }

    pub fn identity_continuity_grants(
        &self,
        actor: &PrincipalId,
        domain_id: &str,
    ) -> Result<Vec<IdentityContinuityGrantInfo>> {
        let c = self.connection()?;
        let (user, admin) = identity_metadata_scope(&c, actor, domain_id)?;
        let ids = if admin {
            let mut q=db(c.prepare("SELECT grant_id FROM identity_continuity_grants WHERE domain_id=?1 ORDER BY accepted_at,grant_id LIMIT 128"))?;
            let rows = db(q.query_map([domain_id], |r| r.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        } else {
            let mut q=db(c.prepare("SELECT grant_id FROM identity_continuity_grants WHERE domain_id=?1 AND accepted_by_user_id=?2 ORDER BY accepted_at,grant_id LIMIT 128"))?;
            let rows = db(q.query_map(params![domain_id, user], |r| r.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        ids.iter().map(|id| grant_row(&c, id)).collect()
    }

    pub fn revoke_identity_continuity_grant(
        &self,
        actor: &PrincipalId,
        grant_id: &str,
    ) -> Result<IdentityContinuityGrantInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let grant = grant_row(&tx, grant_id)?;
        let user = user_for_principal(&tx, actor)?.ok_or(Error::Denied(
            "identity continuity grant revocation requires an active known user",
        ))?;
        let admin: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active' AND role IN ('owner','admin'))",
            params![grant.domain_id,user],
            |r| r.get(0),
        ))?;
        if grant.accepted_by_user_id != user && !admin {
            return Err(Error::Denied(
                "identity continuity grant can be revoked only by its accepter or a Domain owner/admin",
            ));
        }
        if grant.status == "active" {
            db(tx.execute(
                "UPDATE identity_continuity_grants SET status='revoked',revision=revision+1,revoked_at=?1 WHERE grant_id=?2 AND status='active'",
                params![now(),grant_id],
            ))?;
            db(tx.execute(
                "UPDATE portable_identity_bindings SET status='revoked' WHERE domain_id=?1 AND source_node_id=?2 AND status='active'",
                params![grant.domain_id,grant.source_node_id],
            ))?;
            security_audit(
                &tx,
                &user,
                "identity.continuity_source_revoked",
                "domain",
                Some(&grant.domain_id),
                serde_json::json!({"source_node_id":grant.source_node_id,"grant_id":grant_id}),
            )?;
        }
        db(tx.commit())?;
        grant_row(&c, grant_id)
    }

    pub fn export_identity_continuity_snapshot(
        &self,
        actor: &PrincipalId,
        domain_id: &str,
        target_node_id: &str,
    ) -> Result<IdentityContinuitySnapshot> {
        let peer = self.awnet_peer_by_node_id(target_node_id)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied(
                "identity continuity target peer is not trusted",
            ));
        }
        let c = self.connection()?;
        let _ = require_domain_admin(&c, actor, domain_id)?;
        authority::require_domain_authority_write(&c, domain_id)?;
        let target_valid:bool=db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM machines WHERE node_id=?1 AND owning_domain_id=?2 AND kind='remote' AND status='active')",
            params![target_node_id,domain_id],|r|r.get(0)))?;
        if !target_valid {
            return Err(Error::Denied(
                "identity continuity target must be an active remote Machine in the Domain",
            ));
        }
        let domain_security_epoch: u64 = db(c.query_row(
            "SELECT security_epoch FROM domains WHERE domain_id=?1 AND status='active'",
            [domain_id],
            |r| r.get(0),
        ))?;
        let source_store_id: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='store_id'",
            [],
            |r| r.get(0),
        ))?;
        let source_node_id: String = db(c.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        let mut q=db(c.prepare("SELECT m.user_id FROM domain_memberships m JOIN users u ON u.user_id=m.user_id WHERE m.domain_id=?1 AND m.status='active' AND u.status='active' ORDER BY m.user_id LIMIT ?2"))?;
        let rows = db(
            q.query_map(params![domain_id, (MAX_CONTINUITY_USERS + 1) as i64], |r| {
                r.get::<_, String>(0)
            }),
        )?;
        let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if rows.len() > MAX_CONTINUITY_USERS {
            return Err(Error::Limit(
                "identity continuity Domain member bound exceeded",
            ));
        }
        let mut users = Vec::with_capacity(rows.len());
        for user_id in rows {
            let user = user_row(&c, &user_id)?;
            let mut bindings = Vec::new();
            let mut dq=db(c.prepare("SELECT device_id,public_key,registered_at FROM user_devices WHERE user_id=?1 AND status='active' AND public_key IS NOT NULL ORDER BY device_id LIMIT ?2"))?;
            let device_rows = db(dq.query_map(
                params![user_id, (MAX_BINDINGS_PER_USER + 1) as i64],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            ))?;
            for item in device_rows {
                let (device_id, public_key, verified_at) = db(item)?;
                let public_key = canonical_public_key(&public_key)?;
                let binding_key = device_binding_key(&public_key)?;
                bindings.push(PortableIdentityDescriptor {
                    kind: "device_key".into(),
                    binding_sha256: binding_descriptor_hash(
                        "device_key",
                        &binding_key,
                        Some(&device_id),
                        Some(&public_key),
                        None,
                        None,
                        None,
                        Some(verified_at),
                    )?,
                    binding_key,
                    device_id: Some(device_id),
                    public_key: Some(public_key),
                    provider_id: None,
                    issuer: None,
                    subject: None,
                    verified_at: Some(verified_at),
                });
            }
            let mut oq=db(c.prepare("SELECT identity_id,provider_id,issuer,subject,verified_at FROM user_identities WHERE user_id=?1 AND kind='oidc' AND status='active' ORDER BY identity_id LIMIT ?2"))?;
            let oidc_rows = db(oq.query_map(
                params![user_id, (MAX_BINDINGS_PER_USER + 1) as i64],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                    ))
                },
            ))?;
            for item in oidc_rows {
                let (_identity_id, provider_id, issuer, subject, verified_at) = db(item)?;
                let binding_key = oidc_binding_key(&issuer, &subject)?;
                bindings.push(PortableIdentityDescriptor {
                    kind: "oidc".into(),
                    binding_sha256: binding_descriptor_hash(
                        "oidc",
                        &binding_key,
                        None,
                        None,
                        provider_id.as_deref(),
                        Some(&issuer),
                        Some(&subject),
                        verified_at,
                    )?,
                    binding_key,
                    device_id: None,
                    public_key: None,
                    provider_id,
                    issuer: Some(issuer),
                    subject: Some(subject),
                    verified_at,
                });
            }
            if bindings.len() > MAX_BINDINGS_PER_USER {
                return Err(Error::Limit(
                    "identity continuity bindings per user exceed 32",
                ));
            }
            users.push(IdentityContinuityUser { user, bindings });
        }
        let mut snapshot = IdentityContinuitySnapshot {
            version: IDENTITY_CONTINUITY_VERSION,
            source_store_id,
            source_node_id,
            domain_id: domain_id.into(),
            domain_security_epoch,
            users,
            snapshot_sha256: String::new(),
        };
        snapshot.snapshot_sha256 = snapshot_hash(&snapshot)?;
        if json(&snapshot)?.len() > MAX_CONTINUITY_BYTES {
            return Err(Error::Limit("identity continuity snapshot exceeds 1 MiB"));
        }
        Ok(snapshot)
    }

    pub fn import_identity_continuity_snapshot(
        &self,
        actor: &PrincipalId,
        authenticated_source_node: &str,
        snapshot: &IdentityContinuitySnapshot,
        source_signature: &str,
    ) -> Result<IdentityContinuityApplyResult> {
        if snapshot.version != IDENTITY_CONTINUITY_VERSION
            || snapshot.source_node_id != authenticated_source_node
            || snapshot.users.len() > MAX_CONTINUITY_USERS
            || snapshot_hash(snapshot)? != snapshot.snapshot_sha256
            || json(snapshot)?.len() > MAX_CONTINUITY_BYTES
        {
            return Err(Error::Protocol(
                "invalid identity continuity snapshot envelope",
            ));
        }
        let peer = self.awnet_peer_by_node_id(authenticated_source_node)?;
        if peer.trust_state != "trusted" {
            return Err(Error::Denied(
                "identity continuity source peer is not trusted",
            ));
        }
        verify_snapshot_signature(&peer.node_id, &peer.public_key, snapshot, source_signature)?;
        let expected_bindings = validate_snapshot_users(snapshot)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let actor_user = user_for_principal(&tx, actor)?.ok_or(Error::Denied(
            "identity continuity import requires an active known local user",
        ))?;
        let grant_id:Option<String>=db(tx.query_row("SELECT grant_id FROM identity_continuity_grants WHERE domain_id=?1 AND source_node_id=?2 AND status='active'",params![snapshot.domain_id,authenticated_source_node],|r|r.get(0)).optional())?;
        let Some(grant_id) = grant_id else {
            return Err(Error::Denied(
                "identity continuity source has not been explicitly accepted",
            ));
        };
        let grant = grant_row(&tx, &grant_id)?;
        require_identity_import_actor(&tx, &actor_user, &grant)?;
        let local_domain_epoch: u64 = db(tx.query_row(
            "SELECT security_epoch FROM domains WHERE domain_id=?1",
            [&snapshot.domain_id],
            |r| r.get(0),
        ))?;
        if snapshot.domain_security_epoch < local_domain_epoch {
            return Err(Error::Denied(
                "stale identity continuity Domain security epoch",
            ));
        }
        if snapshot.domain_security_epoch > local_domain_epoch {
            let replica_only: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM workspace_replica_domains WHERE domain_id=?1)",
                [&snapshot.domain_id],
                |r| r.get(0),
            ))?;
            if !replica_only {
                return Err(Error::Conflict(
                    "identity continuity snapshot is ahead of local authoritative Domain state",
                ));
            }
            db(tx.execute(
                "UPDATE domains SET security_epoch=?1,updated_at=?2 WHERE domain_id=?3",
                params![snapshot.domain_security_epoch, now(), snapshot.domain_id],
            ))?;
        }
        let existing_snapshot:Option<String>=db(tx.query_row("SELECT snapshot_id FROM identity_continuity_snapshots WHERE domain_id=?1 AND source_store_id=?2 AND snapshot_sha256=?3",params![snapshot.domain_id,snapshot.source_store_id,snapshot.snapshot_sha256],|r|r.get(0)).optional())?;
        let reused = existing_snapshot.is_some();
        if let Some(snapshot_id) = existing_snapshot.as_ref() {
            if projection_matches_snapshot(
                &tx,
                snapshot,
                authenticated_source_node,
                &expected_bindings,
            )? {
                return Ok(IdentityContinuityApplyResult {
                    snapshot_id: snapshot_id.clone(),
                    domain_id: snapshot.domain_id.clone(),
                    users: snapshot.users.len(),
                    active_bindings: expected_bindings.len(),
                    reused: true,
                });
            }
        }
        db(tx.execute("UPDATE portable_identity_bindings SET status='revoked' WHERE domain_id=?1 AND source_node_id=?2 AND status='active'",params![snapshot.domain_id,authenticated_source_node]))?;
        for entry in &snapshot.users {
            let existing: Option<(String, i64)> = db(tx
                .query_row(
                    "SELECT realm_id,created_at FROM users WHERE user_id=?1",
                    [&entry.user.user_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional())?;
            if let Some((realm, created_at)) = existing {
                if realm != entry.user.realm_id || created_at != entry.user.created_at {
                    return Err(Error::Conflict(
                        "stable aw user ID collides with different immutable origin metadata",
                    ));
                }
                let local_epoch: u64 = db(tx.query_row(
                    "SELECT security_epoch FROM users WHERE user_id=?1",
                    [&entry.user.user_id],
                    |r| r.get(0),
                ))?;
                if entry.user.security_epoch < local_epoch {
                    return Err(Error::Denied(
                        "stale stable-user security epoch in identity continuity snapshot",
                    ));
                }
                db(tx.execute(
                    "UPDATE users SET status=?1,display_json=?2,security_epoch=?3 WHERE user_id=?4",
                    params![
                        entry.user.status,
                        json(&entry.user.display)?,
                        entry.user.security_epoch,
                        entry.user.user_id
                    ],
                ))?;
            } else {
                db(tx.execute("INSERT INTO users(user_id,realm_id,status,display_json,security_epoch,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![entry.user.user_id,entry.user.realm_id,entry.user.status,json(&entry.user.display)?,entry.user.security_epoch,entry.user.created_at]))?;
            }
            for binding in &entry.bindings {
                let existing:Option<(String,String)>=db(tx.query_row("SELECT binding_id,user_id FROM portable_identity_bindings WHERE domain_id=?1 AND kind=?2 AND binding_key=?3",params![snapshot.domain_id,binding.kind,binding.binding_key],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
                let binding_id = if let Some((binding_id, _)) = existing.as_ref() {
                    binding_id.clone()
                } else {
                    portable_binding_id(&snapshot.domain_id, &binding.kind, &binding.binding_key)?
                };
                if existing.as_ref().is_some_and(|v| v.1 != entry.user.user_id) {
                    return Err(Error::Conflict(
                        "portable identity binding is already linked to another stable aw user",
                    ));
                }
                db(tx.execute("INSERT INTO portable_identity_bindings(binding_id,domain_id,user_id,source_store_id,source_node_id,kind,binding_key,device_id,public_key,provider_id,issuer,subject,user_security_epoch,domain_security_epoch,verified_at,binding_sha256,status,imported_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,'active',?17) ON CONFLICT(binding_id) DO UPDATE SET source_store_id=excluded.source_store_id,source_node_id=excluded.source_node_id,user_security_epoch=excluded.user_security_epoch,domain_security_epoch=excluded.domain_security_epoch,verified_at=excluded.verified_at,binding_sha256=excluded.binding_sha256,status='active',imported_at=excluded.imported_at",params![binding_id,snapshot.domain_id,entry.user.user_id,snapshot.source_store_id,authenticated_source_node,binding.kind,binding.binding_key,binding.device_id,binding.public_key,binding.provider_id,binding.issuer,binding.subject,entry.user.security_epoch,snapshot.domain_security_epoch,binding.verified_at,binding.binding_sha256,now()]))?;
            }
        }
        let snapshot_id = existing_snapshot
            .unwrap_or_else(|| format!("icsnap_{}", &snapshot.snapshot_sha256[..32]));
        if !reused {
            db(tx.execute("INSERT INTO identity_continuity_snapshots(snapshot_id,domain_id,source_store_id,source_node_id,domain_security_epoch,snapshot_sha256,body_json,source_signature,imported_by_grant_id,imported_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![snapshot_id,snapshot.domain_id,snapshot.source_store_id,authenticated_source_node,snapshot.domain_security_epoch,snapshot.snapshot_sha256,json(snapshot)?,source_signature,grant_id,now()]))?;
        }
        security_audit(
            &tx,
            &actor_user,
            "identity.continuity_snapshot_applied",
            "domain",
            Some(&snapshot.domain_id),
            serde_json::json!({"source_node_id":authenticated_source_node,"snapshot_sha256":snapshot.snapshot_sha256,"users":snapshot.users.len()}),
        )?;
        let active = count_active_bindings(&tx, &snapshot.domain_id, authenticated_source_node)?;
        db(tx.commit())?;
        Ok(IdentityContinuityApplyResult {
            snapshot_id,
            domain_id: snapshot.domain_id.clone(),
            users: snapshot.users.len(),
            active_bindings: active,
            reused,
        })
    }

    pub fn portable_identity_bindings(
        &self,
        actor: &PrincipalId,
        domain_id: &str,
    ) -> Result<Vec<PortableIdentityBindingInfo>> {
        let c = self.connection()?;
        let (user, admin) = identity_metadata_scope(&c, actor, domain_id)?;
        let ids = if admin {
            let mut q=db(c.prepare("SELECT binding_id FROM portable_identity_bindings WHERE domain_id=?1 ORDER BY user_id,kind,binding_id LIMIT 2048"))?;
            let rows = db(q.query_map([domain_id], |r| r.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        } else {
            let mut q=db(c.prepare("SELECT b.binding_id FROM portable_identity_bindings b JOIN identity_continuity_grants g ON g.domain_id=b.domain_id AND g.source_node_id=b.source_node_id WHERE b.domain_id=?1 AND g.accepted_by_user_id=?2 ORDER BY b.user_id,b.kind,b.binding_id LIMIT 2048"))?;
            let rows = db(q.query_map(params![domain_id, user], |r| r.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        ids.iter().map(|id| binding_row(&c, id)).collect()
    }

    pub fn authenticate_portable_device_signature(
        &self,
        domain_id: &str,
        public_key: &str,
        transcript: &[u8],
        signature_b64: &str,
    ) -> Result<AuthenticatedStableUser> {
        let c = self.connection()?;
        authenticate_portable_device_signature_in(
            &c,
            domain_id,
            public_key,
            transcript,
            signature_b64,
        )
    }
}

/// AW-P6-10U: original device signature verification on an independently
/// authenticated, owner-private READ-ONLY SQLite v33 Connection. This does NOT
/// open a Store, register a Process or grant any runnable Tool/Actor authority.
/// The old SessionStore method delegates to this SAME original predicate.
pub(crate) fn authenticate_portable_device_signature_in(
    c: &Connection,
    domain_id: &str,
    public_key: &str,
    transcript: &[u8],
    signature_b64: &str,
) -> Result<AuthenticatedStableUser> {
    if transcript.is_empty() || transcript.len() > 4096 {
        return Err(Error::Protocol(
            "identity proof transcript must be 1..4096 bytes",
        ));
    }
    let public_key = canonical_public_key(public_key)?;
    let binding_key = device_binding_key(&public_key)?;
    let row:Option<(String,String,String,u64,u64)>=db(c.query_row(
        "SELECT b.binding_id,b.user_id,b.source_node_id,b.user_security_epoch,b.domain_security_epoch FROM portable_identity_bindings b JOIN identity_continuity_grants g ON g.domain_id=b.domain_id AND g.source_node_id=b.source_node_id AND g.status='active' JOIN users u ON u.user_id=b.user_id JOIN domains d ON d.domain_id=b.domain_id WHERE b.domain_id=?1 AND b.kind='device_key' AND b.binding_key=?2 AND b.public_key=?3 AND b.status='active' AND u.status='active' AND u.security_epoch=b.user_security_epoch AND d.status='active' AND d.security_epoch=b.domain_security_epoch",
        params![domain_id,binding_key,public_key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional())?;
    let Some((binding_id, user_id, _source, user_epoch, domain_epoch)) = row else {
        return Err(Error::Denied(
            "portable identity device binding is unavailable, stale or revoked",
        ));
    };
    let verifying = VerifyingKey::from_bytes(&decode_public_key(&public_key)?)
        .map_err(|_| Error::Protocol("invalid portable identity Ed25519 key"))?;
    let raw = STANDARD
        .decode(signature_b64)
        .map_err(|_| Error::Protocol("invalid portable identity proof signature"))?;
    let raw: [u8; 64] = raw
        .try_into()
        .map_err(|_| Error::Protocol("portable identity proof signature must be 64 bytes"))?;
    verifying
        .verify_strict(transcript, &Signature::from_bytes(&raw))
        .map_err(|_| Error::Denied("portable identity device proof failed"))?;
    let role:Option<String>=db(c.query_row("SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",params![domain_id,user_id],|r|r.get(0)).optional())?;
    Ok(AuthenticatedStableUser {
        user_id: user_id.clone(),
        stable_principal: stable_user_principal(&user_id)?.as_str().to_owned(),
        domain_id: domain_id.into(),
        domain_role: role,
        user_security_epoch: user_epoch,
        domain_security_epoch: domain_epoch,
        method: "device_key".into(),
        binding_id,
    })
}

fn require_identity_import_actor(
    c: &Connection,
    actor_user_id: &str,
    grant: &IdentityContinuityGrantInfo,
) -> Result<()> {
    if grant.accepted_by_user_id == actor_user_id
        || user_is_domain_admin(c, actor_user_id, &grant.domain_id)?
    {
        Ok(())
    } else {
        Err(Error::Denied(
            "identity continuity import requires the exact grant accepter or a Domain owner/admin",
        ))
    }
}

fn identity_metadata_scope(
    c: &Connection,
    actor: &PrincipalId,
    domain_id: &str,
) -> Result<(String, bool)> {
    let user = user_for_principal(c, actor)?.ok_or(Error::Denied(
        "identity continuity metadata unavailable or not permitted",
    ))?;
    let admin = user_is_domain_admin(c, &user, domain_id)?;
    if !admin {
        let accepted: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM identity_continuity_grants WHERE domain_id=?1 AND accepted_by_user_id=?2)",
            params![domain_id,user],
            |r| r.get(0),
        ))?;
        if !accepted {
            return Err(Error::Denied(
                "identity continuity metadata unavailable or not permitted",
            ));
        }
    }
    Ok((user, admin))
}

fn validate_descriptor(binding: &PortableIdentityDescriptor) -> Result<()> {
    match binding.kind.as_str() {
        "device_key" => {
            let public = binding.public_key.as_deref().ok_or(Error::Protocol(
                "device identity binding missing public key",
            ))?;
            let device = binding
                .device_id
                .as_deref()
                .ok_or(Error::Protocol("device identity binding missing device ID"))?;
            label(device, 128)?;
            if binding.binding_key != device_binding_key(public)?
                || binding.issuer.is_some()
                || binding.subject.is_some()
            {
                return Err(Error::Conflict(
                    "invalid device portable identity descriptor",
                ));
            }
        }
        "oidc" => {
            let issuer = binding
                .issuer
                .as_deref()
                .ok_or(Error::Protocol("OIDC identity binding missing issuer"))?;
            let subject = binding
                .subject
                .as_deref()
                .ok_or(Error::Protocol("OIDC identity binding missing subject"))?;
            if binding.binding_key != oidc_binding_key(issuer, subject)?
                || binding.public_key.is_some()
            {
                return Err(Error::Conflict("invalid OIDC portable identity descriptor"));
            }
        }
        _ => return Err(Error::Protocol("unsupported portable identity kind")),
    }
    Ok(())
}

fn count_active_bindings(c: &Connection, domain_id: &str, source_node_id: &str) -> Result<usize> {
    let count:u64=db(c.query_row("SELECT count(*) FROM portable_identity_bindings WHERE domain_id=?1 AND source_node_id=?2 AND status='active'",params![domain_id,source_node_id],|r|r.get(0)))?;
    usize::try_from(count).map_err(|_| Error::Limit("portable identity binding count overflow"))
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn unrelated_continuity_grant_does_not_authorize_import() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("unix:1000").unwrap();
        let other = PrincipalId::new("internal:fixture:other").unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let owner_user = store.current_user(&owner).unwrap();
        let other_user = store.current_user(&other).unwrap();
        let grant = IdentityContinuityGrantInfo {
            id: "icgrant_fixture".into(),
            domain_id: domain.id,
            source_node_id: "node_source_b".into(),
            status: "active".into(),
            accepted_by_user_id: owner_user.user_id.clone(),
            revision: 1,
            accepted_at: 1,
            revoked_at: None,
        };
        let c = store.connection().unwrap();
        assert!(require_identity_import_actor(&c, &owner_user.user_id, &grant).is_ok());
        assert!(matches!(
            require_identity_import_actor(&c, &other_user.user_id, &grant),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn portable_binding_row_ids_are_domain_scoped() {
        let key = "device:fixture";
        let a = portable_binding_id("dom_a", "device_key", key).unwrap();
        let b = portable_binding_id("dom_b", "device_key", key).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, portable_binding_id("dom_a", "device_key", key).unwrap());
    }

    #[test]
    fn non_admin_identity_metadata_is_scoped_to_sources_the_user_accepted() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("unix:1000").unwrap();
        let a = PrincipalId::new("internal:fixture:a").unwrap();
        let b = PrincipalId::new("internal:fixture:b").unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let a_user = store.current_user(&a).unwrap();
        let b_user = store.current_user(&b).unwrap();
        {
            let c = store.connection().unwrap();
            c.execute(
                "INSERT INTO identity_continuity_grants(grant_id,domain_id,source_node_id,status,accepted_by_user_id,revision,accepted_at) VALUES('g_a',?1,'node_a','active',?2,1,1),('g_b',?1,'node_b','revoked',?3,2,2)",
                params![domain.id,a_user.user_id,b_user.user_id],
            )
            .unwrap();
            c.execute(
                "INSERT INTO portable_identity_bindings(binding_id,domain_id,user_id,source_store_id,source_node_id,kind,binding_key,device_id,public_key,user_security_epoch,domain_security_epoch,binding_sha256,status,imported_at) VALUES('p_a',?1,?2,'store_a','node_a','device_key','device:a','dev_a',?4,1,1,?5,'active',1),('p_b',?1,?3,'store_b','node_b','device_key','device:b','dev_b',?6,1,1,?7,'revoked',2)",
                params![domain.id,a_user.user_id,b_user.user_id,"11".repeat(32),"a".repeat(64),"22".repeat(32),"b".repeat(64)],
            )
            .unwrap();
        }

        let a_grants = store.identity_continuity_grants(&a, &domain.id).unwrap();
        assert_eq!(a_grants.len(), 1);
        assert_eq!(a_grants[0].source_node_id, "node_a");
        let a_bindings = store.portable_identity_bindings(&a, &domain.id).unwrap();
        assert_eq!(a_bindings.len(), 1);
        assert_eq!(a_bindings[0].source_node_id, "node_a");

        let b_grants = store.identity_continuity_grants(&b, &domain.id).unwrap();
        assert_eq!(b_grants.len(), 1);
        assert_eq!(b_grants[0].source_node_id, "node_b");
        let b_bindings = store.portable_identity_bindings(&b, &domain.id).unwrap();
        assert_eq!(b_bindings.len(), 1);
        assert_eq!(b_bindings[0].source_node_id, "node_b");

        assert_eq!(
            store
                .identity_continuity_grants(&owner, &domain.id)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            store
                .portable_identity_bindings(&owner, &domain.id)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn metadata_read_authorization_does_not_create_unknown_user() {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("unix:1000").unwrap();
        let domain = store.default_domain(&owner).unwrap();
        let stranger = PrincipalId::new("internal:fixture:stranger").unwrap();
        let before = {
            let c = store.connection().unwrap();
            c.query_row("SELECT count(*) FROM users", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        assert!(
            identity_metadata_scope(&store.connection().unwrap(), &stranger, &domain.id).is_err()
        );
        let after = {
            let c = store.connection().unwrap();
            c.query_row("SELECT count(*) FROM users", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        assert_eq!(before, after);
    }
}
