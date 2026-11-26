//! Agent-owned Vault references. Provider authority and secret custody
//! remain external; persisted evidence never authorizes a projection.
use crate::{
    agent_owner::{self, AgentKey, AgentPermission},
    db, durability,
    store_scope::StoreScope,
    user_for_principal, workspace_writer, Error, Result, SessionStore, StoreMode,
};
use aw_agent_core::{
    PrincipalId, SourceName, VaultEntryRef, VaultProjection, VaultProviderRegistryReader,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_GRANTS: usize = 128;

pub(crate) fn opaque(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(Error::Config(
            "invalid opaque Vault authority or scope identity".into(),
        ));
    }
    Ok(())
}

/// Exact independently authorized subject and logical entry/projection.
/// Scope is a provider-owned non-secret identifier, not an arbitrary target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultGrantTarget {
    pub agent_ref: String,
    pub name: SourceName,
    pub entry: VaultEntryRef,
    pub projection: VaultProjection,
    pub scope: String,
    pub actor: PrincipalId,
}

impl VaultGrantTarget {
    fn validate(&self) -> Result<()> {
        crate::label(&self.agent_ref, 2048)?;
        self.name.validate()?;
        self.entry.validate()?;
        opaque(&self.scope)
    }
}

/// Immutable, nonsecret commitment to one selected Process projection.
/// This is potential exposure provenance, not a Vault grant or proof
/// that materialization/spawn actually occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultProcessProjectionClaim {
    projection: VaultProjection,
    selection_digest: [u8; 32],
}

impl VaultProcessProjectionClaim {
    pub fn new(
        entry: &VaultEntryRef,
        name: &SourceName,
        scope: &str,
        grant_rev: u64,
        projection: VaultProjection,
    ) -> Result<Self> {
        use sha2::{Digest as _, Sha256};
        entry.validate()?;
        name.validate()?;
        opaque(scope)?;
        if projection == VaultProjection::Broker || grant_rev == 0 || grant_rev > i64::MAX as u64 {
            return Err(Error::Denied(
                "invalid original Vault Process projection selection",
            ));
        }
        let mut hash = Sha256::new();
        hash.update(b"aw-vault-process-projection-claim-v1\0");
        hash.update(crate::json(&(entry, name, scope, grant_rev, projection))?.as_bytes());
        Ok(Self {
            projection,
            selection_digest: hash.finalize().into(),
        })
    }

    pub(crate) fn for_target(target: &VaultGrantTarget, grant_rev: u64) -> Result<Self> {
        Self::new(
            &target.entry,
            &target.name,
            &target.scope,
            grant_rev,
            target.projection,
        )
    }
}

/// Evidence supplied by a trusted provider's live authority owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultGrantPolicy {
    pub target: VaultGrantTarget,
    pub authority_ref: String,
    pub authority_generation: u64,
    actor_user_id: String,
    store_id: String,
    restore_generation: u64,
}

impl VaultGrantPolicy {
    pub(crate) fn store_id(&self) -> &str {
        &self.store_id
    }

    pub(crate) fn restore_generation(&self) -> u64 {
        self.restore_generation
    }

    fn validate(&self) -> Result<()> {
        self.target.validate()?;
        opaque(&self.authority_ref)?;
        crate::stable_user_principal(&self.actor_user_id)?;
        if self.authority_generation == 0
            || self.authority_generation > i64::MAX as u64
            || self.restore_generation == 0
            || self.store_id.is_empty()
        {
            return Err(Error::Conflict("invalid Vault authority generation"));
        }
        Ok(())
    }

    fn require_scope(&self, c: &Connection) -> Result<()> {
        let scope = StoreScope::load(c)?;
        if self.store_id != scope.id().as_str()
            || self.restore_generation != scope.restore_generation()
            || user_for_principal(c, &self.target.actor)?.as_deref() != Some(&self.actor_user_id)
        {
            return Err(Error::Denied(
                "Vault authority subject or Store scope changed",
            ));
        }
        Ok(())
    }
}

/// Trusted provider guard, held through the actual Store commit or broker
/// operation. It must exclude withdrawal/rotation for this exact decision.
/// A copy of durable policy evidence alone is not an implementation.
pub trait VaultGrantAuthority {
    fn policy(&self) -> &VaultGrantPolicy;
    fn authorize_attach(&self, manager: &PrincipalId, manager_user_id: &str) -> Result<()>;
    fn authorize_use(&self) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultGrantStatus {
    RevalidationRequired,
    Stale,
}

/// Deliberately redacted metadata. No provider, entry or authority locator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultGrantSummary {
    pub name: SourceName,
    pub projection: VaultProjection,
    pub grant_rev: u64,
    pub status: VaultGrantStatus,
}

/// Metadata-only view: the redacted binding summary is always present;
/// an entry reference requires a separate, live provider disclosure decision.
/// Scope, authority locator, backend/key locator and credential bytes are
/// deliberately not part of this representation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VaultGrantView {
    #[serde(flatten)]
    pub summary: VaultGrantSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<VaultEntryRef>,
}

#[derive(PartialEq, Eq)]
struct Grant {
    policy: VaultGrantPolicy,
    rev: u64,
    access_generation: [u8; 32],
}

fn projection_name(kind: VaultProjection) -> &'static str {
    match kind {
        VaultProjection::Broker => "broker",
        VaultProjection::Socket => "socket",
        VaultProjection::File => "file",
        VaultProjection::Env => "env",
    }
}

fn projection(value: &str) -> Result<VaultProjection> {
    match value {
        "broker" => Ok(VaultProjection::Broker),
        "socket" => Ok(VaultProjection::Socket),
        "file" => Ok(VaultProjection::File),
        "env" => Ok(VaultProjection::Env),
        _ => Err(Error::Storage("invalid durable Vault projection")),
    }
}

fn exact_key(c: &Connection, reference: &str) -> Result<(String, String)> {
    let (session, branch) = agent_owner::key_for_ref(c, reference)?;
    if agent_owner::canonical_ref(c, AgentKey::new(&session, &branch))? != reference {
        return Err(Error::Denied("Vault requires an exact canonical Agent"));
    }
    Ok((session, branch))
}

fn grant_in(
    c: &Connection,
    key: AgentKey<'_>,
    reference: &str,
    name: &SourceName,
) -> Result<Option<Grant>> {
    let mut query = db(c.prepare_cached(
        "SELECT grant_rev,provider_id,entry_ref,projection,scope,authority_ref,
                authority_generation,actor_principal,actor_user_id,access_generation,
                store_id,restore_generation
         FROM agent_vault_grants_current WHERE session_id=?1 AND branch_id=?2 AND name=?3",
    ))?;
    let mut rows = db(query.query(params![key.session, key.branch, name.as_str()]))?;
    let Some(row) = db(rows.next())? else {
        return Ok(None);
    };
    let grant = Grant {
        rev: db(row.get(0))?,
        policy: VaultGrantPolicy {
            target: VaultGrantTarget {
                agent_ref: reference.to_owned(),
                name: name.clone(),
                entry: VaultEntryRef {
                    provider: crate::decode(&db(row.get::<_, String>(1))?)?,
                    entry: db(row.get(2))?,
                },
                projection: projection(&db(row.get::<_, String>(3))?)?,
                scope: db(row.get(4))?,
                actor: PrincipalId::new(db(row.get::<_, String>(7))?)?,
            },
            authority_ref: db(row.get(5))?,
            authority_generation: db(row.get(6))?,
            actor_user_id: db(row.get(8))?,
            store_id: db(row.get(10))?,
            restore_generation: db(row.get(11))?,
        },
        access_generation: db(row.get(9))?,
    };
    grant.policy.validate()?;
    if grant.rev == 0 {
        return Err(Error::Storage("invalid durable Vault grant revision"));
    }
    Ok(Some(grant))
}

impl Grant {
    fn require_current(&self, c: &Connection, key: AgentKey<'_>) -> Result<()> {
        self.policy.require_scope(c)?;
        let current = agent_owner::authorize_permission(
            c,
            &self.policy.target.actor,
            key,
            AgentPermission::Submit,
        )?;
        if current.access_generation != self.access_generation {
            return Err(Error::Denied("Vault grant access generation changed"));
        }
        Ok(())
    }
}

/// The existing grant owner checks an exact live provider decision under
/// the caller's original admission transaction. Actor/Agent/site authority
/// and any additional effect-specific gates remain with that caller.
pub(crate) fn require_projection_in(
    c: &Connection,
    registry: &VaultProviderRegistryReader,
    authority: &impl VaultGrantAuthority,
    expected_grant_rev: u64,
    projection: VaultProjection,
) -> Result<()> {
    let policy = authority.policy();
    let target = &policy.target;
    if target.projection != projection {
        return Err(Error::Denied(
            "Vault projection differs from authorized kind",
        ));
    }
    registry.require_projection(&target.entry, projection)?;
    let (session, branch) = exact_key(c, &target.agent_ref)?;
    let key = AgentKey::new(&session, &branch);
    let grant = grant_in(c, key, &target.agent_ref, &target.name)?
        .filter(|grant| grant.rev == expected_grant_rev && grant.policy == *policy)
        .ok_or(Error::Denied("Vault grant unavailable or changed"))?;
    grant.require_current(c, key)?;
    authority.authorize_use()
}

fn set_in(
    tx: &Connection,
    manager: &PrincipalId,
    key: AgentKey<'_>,
    reference: &str,
    name: &SourceName,
    expected_rev: u64,
    authority: Option<(&VaultProviderRegistryReader, &dyn VaultGrantAuthority)>,
) -> Result<u64> {
    let session = key.session;
    let branch = key.branch;
    agent_owner::authorize_permission(tx, manager, key, AgentPermission::Manage)?;
    let before = agent_owner::revision(tx, key)?;
    if before != expected_rev {
        return Err(Error::Conflict("stale Agent revision"));
    }
    workspace_writer::require_workspace_writer_for_session(tx, session)?;
    let current = grant_in(tx, key, reference, name)?;
    if current.as_ref().is_some_and(|grant| grant.rev > before) {
        return Err(Error::Storage(
            "Vault grant revision exceeds Agent revision",
        ));
    }
    let next = before
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(Error::Limit("Agent revision exhausted"))?;
    if let Some((registry, authority)) = authority {
        if agent_owner::lifecycle(tx, key)?.0 != "open" {
            return Err(Error::Denied("Vault attach requires an open Agent"));
        }
        let policy = authority.policy();
        policy.validate()?;
        policy.require_scope(tx)?;
        registry.require_projection(&policy.target.entry, policy.target.projection)?;
        if policy.target.agent_ref != reference || &policy.target.name != name {
            return Err(Error::Denied("Vault approval belongs to another binding"));
        }
        let manager_user =
            user_for_principal(tx, manager)?.ok_or(Error::Denied("Vault manager unavailable"))?;
        authority.authorize_attach(manager, &manager_user)?;
        let access = agent_owner::authorize_permission(
            tx,
            &policy.target.actor,
            key,
            AgentPermission::Submit,
        )?;
        let generation = access.access_generation;
        if current
            .as_ref()
            .is_some_and(|g| g.policy == *policy && g.access_generation == generation)
        {
            return Ok(before);
        }
        if current.is_none() {
            let count: usize = db(tx.query_row(
                "SELECT count(*) FROM agent_vault_grants_current WHERE session_id=?1 AND branch_id=?2",
                params![session, branch], |row| row.get(0)))?;
            if count >= MAX_GRANTS {
                return Err(Error::Limit("too many Agent Vault grants"));
            }
        }
        let target = &policy.target;
        db(tx.execute(
            "INSERT INTO agent_vault_grants_current(session_id,branch_id,name,grant_rev,
              provider_id,entry_ref,projection,scope,authority_ref,authority_generation,
              actor_principal,actor_user_id,access_generation,store_id,restore_generation)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(session_id,branch_id,name) DO UPDATE SET
              grant_rev=excluded.grant_rev,provider_id=excluded.provider_id,entry_ref=excluded.entry_ref,
              projection=excluded.projection,scope=excluded.scope,authority_ref=excluded.authority_ref,
              authority_generation=excluded.authority_generation,actor_principal=excluded.actor_principal,
              actor_user_id=excluded.actor_user_id,access_generation=excluded.access_generation,
              store_id=excluded.store_id,restore_generation=excluded.restore_generation",
            params![session, branch, name.as_str(), next, crate::json(&target.entry.provider)?,
                target.entry.entry, projection_name(target.projection), target.scope,
                policy.authority_ref, policy.authority_generation, target.actor.as_str(),
                policy.actor_user_id, generation, policy.store_id, policy.restore_generation],
        ))?;
    } else {
        if current.is_none() {
            return Ok(before);
        }
        db(tx.execute("DELETE FROM agent_vault_grants_current WHERE session_id=?1 AND branch_id=?2 AND name=?3",
            params![session, branch, name.as_str()]))?;
    }
    agent_owner::changed(tx, key)?;
    // Audit presentation contains only the logical binding and factual rev.
    crate::audit(
        tx,
        session,
        manager,
        "agent.vault.changed",
        serde_json::json!({
            "branch_id":branch, "name":name.as_str(), "rev":next, "active":authority.is_some()
        }),
    )?;
    Ok(next)
}

/// Exact logical entry and projection selected by an authenticated host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultBindingSelection {
    pub entry: VaultEntryRef,
    pub projection: VaultProjection,
}

/// Preserve the literal requested ref for replay before contextual resolution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultSetRequest {
    #[serde(rename = "ref")]
    pub reference: String,
    pub name: SourceName,
    pub binding: Option<VaultBindingSelection>,
    pub rev: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum VaultSetOutcome {
    Applied { rev: u64 },
    PriorInvocation,
}

#[derive(Deserialize)]
struct SetFact {
    fingerprint: String,
    reference: String,
    rev: u64,
}

/// One authenticated host request, not an independent authority or ledger.
pub(crate) struct HostVaultSet<'a> {
    pub manager: &'a PrincipalId,
    pub current_ref: &'a str,
    pub request: &'a VaultSetRequest,
    pub request_id: &'a str,
}

impl HostVaultSet<'_> {
    fn identity(&self, c: &Connection) -> Result<(String, String)> {
        crate::label(self.request_id, 256)?;
        crate::label(&self.request.reference, 2048)?;
        self.request.name.validate()?;
        if self.request.rev == 0 || self.request.rev > i64::MAX as u64 {
            return Err(Error::Protocol("invalid Vault Agent revision"));
        }
        if let Some(binding) = &self.request.binding {
            binding.entry.validate()?;
        }
        let user = user_for_principal(c, self.manager)?
            .ok_or(Error::Denied("Vault manager unavailable"))?;
        let mut hash = Sha256::new();
        hash.update(b"aw-vault-set-request-v1\0");
        hash.update(crate::json(&(user, self.request_id))?.as_bytes());
        let event_id = format!("vault-set:{:x}", hash.finalize());
        let mut hash = Sha256::new();
        hash.update(b"aw-vault-set-semantics-v1\0");
        hash.update(crate::json(self.request)?.as_bytes());
        Ok((event_id, format!("{:x}", hash.finalize())))
    }

    fn replay_in(
        &self,
        c: &Connection,
        event_id: &str,
        fingerprint: &str,
    ) -> Result<Option<VaultSetOutcome>> {
        let prior: Option<(String, String, String)> = db(c
            .query_row(
                "SELECT kind,entity_type,payload_json FROM commit_log WHERE event_id=?1",
                [event_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional())?;
        let Some((kind, entity, payload)) = prior else {
            return Ok(None);
        };
        if kind != "agent.vault.settled" || entity != "vault_grant" {
            return Err(Error::Storage("Vault mutation receipt identity changed"));
        }
        let fact: SetFact = crate::decode(&payload)?;
        if fact.fingerprint != fingerprint {
            return Err(Error::Denied(
                "Vault request identity reused with different semantics",
            ));
        }
        // The current host's explicit redacted-metadata capability is limited
        // to its exact Agent. ACL loss cannot revive or disclose the old result.
        if fact.reference != self.current_ref {
            return Ok(Some(VaultSetOutcome::PriorInvocation));
        }
        let access = exact_key(c, &fact.reference).and_then(|(session, branch)| {
            agent_owner::authorize_permission(
                c,
                self.manager,
                AgentKey::new(&session, &branch),
                AgentPermission::Manage,
            )
        });
        match access {
            Ok(_) => Ok(Some(VaultSetOutcome::Applied { rev: fact.rev })),
            Err(Error::Denied(_)) => Ok(Some(VaultSetOutcome::PriorInvocation)),
            Err(error) => Err(error),
        }
    }

    /// Before provider selection/lookup and fresh authority acquisition.
    pub(crate) fn replay(&self, store: &SessionStore) -> Result<Option<VaultSetOutcome>> {
        let mut c = store.connection()?;
        let tx = db(c.transaction())?;
        let (event_id, fingerprint) = self.identity(&tx)?;
        let outcome = self.replay_in(&tx, &event_id, &fingerprint)?;
        if outcome.is_none() {
            let (session, branch) = self.fresh_key(&tx)?;
            let key = AgentKey::new(&session, &branch);
            agent_owner::authorize_permission(&tx, self.manager, key, AgentPermission::Manage)?;
            if agent_owner::revision(&tx, key)? != self.request.rev {
                return Err(Error::Conflict("stale Agent revision"));
            }
        }
        db(tx.commit())?;
        Ok(outcome)
    }

    fn fresh_key(&self, c: &Connection) -> Result<(String, String)> {
        if self.request.reference != "." && self.request.reference != self.current_ref {
            return Err(Error::Denied("Vault host requires the current exact Agent"));
        }
        exact_key(c, self.current_ref)
    }

    /// Recheck replay under the original owner transaction after acquiring
    /// the exact provider guard. One atomic grant CAS + factual receipt.
    pub(crate) fn apply(
        &self,
        store: &SessionStore,
        authority: Option<(&VaultProviderRegistryReader, &dyn VaultGrantAuthority)>,
    ) -> Result<VaultSetOutcome> {
        if store.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "Vault binding requires the exclusive Store owner",
            ));
        }
        let mut c = store.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (event_id, fingerprint) = self.identity(&tx)?;
        if let Some(outcome) = self.replay_in(&tx, &event_id, &fingerprint)? {
            return Ok(outcome);
        }
        let request = self.request;
        let (session, branch) = self.fresh_key(&tx)?;
        match (&request.binding, authority) {
            (None, None) => (),
            (Some(binding), Some((_, guard)))
                if binding.entry == guard.policy().target.entry
                    && binding.projection == guard.policy().target.projection => {}
            _ => {
                return Err(Error::Denied(
                    "Vault provider decision differs from requested binding",
                ))
            }
        }
        let rev = set_in(
            &tx,
            self.manager,
            AgentKey::new(&session, &branch),
            self.current_ref,
            &request.name,
            request.rev,
            authority,
        )?;
        // The original indexed commit-log fact retains even an accepted no-op
        // identity. Store only a digest, canonical target and resulting rev.
        durability::commit_event_position_with_id(
            &tx,
            &event_id,
            &durability::tx_id()?,
            "agent.vault.settled",
            "vault_grant",
            &branch,
            Some(self.manager),
            serde_json::json!({"sync_scope":"local", "fingerprint":fingerprint, "reference":self.current_ref, "rev":rev}),
        )?;
        db(tx.commit())?;
        Ok(VaultSetOutcome::Applied { rev })
    }
}

impl SessionStore {
    /// Bind trusted provider policy to the original Store and actor identity.
    /// This observes identity; it issues no authority and persists nothing.
    pub fn vault_policy(
        &self,
        target: VaultGrantTarget,
        authority_ref: String,
        authority_generation: u64,
    ) -> Result<VaultGrantPolicy> {
        let c = self.connection()?;
        exact_key(&c, &target.agent_ref)?;
        let scope = StoreScope::load(&c)?;
        let actor_user_id = user_for_principal(&c, &target.actor)?
            .ok_or(Error::Denied("Vault actor unavailable"))?;
        let policy = VaultGrantPolicy {
            target,
            authority_ref,
            authority_generation,
            actor_user_id,
            store_id: scope.id().as_str().to_owned(),
            restore_generation: scope.restore_generation(),
        };
        policy.validate()?;
        Ok(policy)
    }

    /// Agent Manage plus the independent live provider attach decision.
    /// Null removes future authority even for closing/closed Agents.
    /// The Agent revision mints grant generations, so remove/re-add cannot
    /// revive a previous grant without keeping separate tombstone rows.
    pub fn agent_vault_set(
        &self,
        manager: &PrincipalId,
        reference: &str,
        name: &SourceName,
        expected_rev: u64,
        authority: Option<(&VaultProviderRegistryReader, &dyn VaultGrantAuthority)>,
    ) -> Result<u64> {
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "Vault binding requires the exclusive Store owner",
            ));
        }
        name.validate()?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = exact_key(&tx, reference)?;
        let rev = set_in(
            &tx,
            manager,
            AgentKey::new(&session, &branch),
            reference,
            name,
            expected_rev,
            authority,
        )?;
        db(tx.commit())?;
        Ok(rev)
    }

    /// Trusted metadata boundary; authorize_metadata independently
    /// authorizes this manager's complete Agent binding metadata and returns
    /// its live guard. Acquire external authority before the Store transaction
    /// (no protected disclosure or effects in that callback). Even then, this
    /// lower view redacts provider identities.
    pub fn agent_vault_list<F>(
        &self,
        manager: &PrincipalId,
        reference: &str,
        authorize_metadata: impl FnOnce(&PrincipalId, &str) -> Result<F>,
    ) -> Result<Vec<VaultGrantSummary>> {
        self.agent_vault_list_projected(manager, reference, authorize_metadata, |_, _, summary| {
            Ok(summary)
        })
    }

    /// The common bounded metadata read. Provider-specific projections
    /// supply their actual live authority outside this read transaction.
    /// The projection callback is synchronous and metadata-only.
    pub(crate) fn agent_vault_list_projected<F, T>(
        &self,
        manager: &PrincipalId,
        reference: &str,
        authorize_metadata: impl FnOnce(&PrincipalId, &str) -> Result<F>,
        mut project: impl FnMut(&str, &VaultGrantPolicy, VaultGrantSummary) -> Result<T>,
    ) -> Result<Vec<T>> {
        let fence = authorize_metadata(manager, reference)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction())?;
        let (session, branch) = exact_key(&tx, reference)?;
        let key = AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&tx, manager, key, AgentPermission::Manage)?;
        let manager_user = user_for_principal(&tx, manager)?
            .ok_or(Error::Denied("Vault metadata manager unavailable"))?;
        let names = {
            let mut q = db(tx.prepare_cached(
                "SELECT name FROM agent_vault_grants_current
                WHERE session_id=?1 AND branch_id=?2 ORDER BY name LIMIT ?3",
            ))?;
            let rows = db(
                q.query_map(params![session, branch, MAX_GRANTS + 1], |row| {
                    row.get::<_, String>(0)
                }),
            )?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        if names.len() > MAX_GRANTS {
            return Err(Error::Limit("Vault binding list bound exceeded"));
        }
        let open = agent_owner::lifecycle(&tx, key)?.0 == "open";
        let mut result = Vec::with_capacity(names.len());
        for name in names {
            let name = SourceName::new(name)?;
            let grant = grant_in(&tx, key, reference, &name)?
                .ok_or(Error::Storage("Vault binding disappeared"))?;
            let status = if open {
                match grant.require_current(&tx, key) {
                    Ok(()) => VaultGrantStatus::RevalidationRequired,
                    Err(Error::Denied(_) | Error::Conflict(_)) => VaultGrantStatus::Stale,
                    Err(error) => return Err(error),
                }
            } else {
                VaultGrantStatus::Stale
            };
            result.push(project(
                &manager_user,
                &grant.policy,
                VaultGrantSummary {
                    name,
                    projection: grant.policy.target.projection,
                    grant_rev: grant.rev,
                    status,
                },
            )?);
        }
        db(tx.commit())?;
        drop(fence);
        Ok(result)
    }

    /// One synchronous local broker operation. The provider guard stays live
    /// and the original Store writer transaction excludes ACL/grant changes
    /// through use. No network wait, Process spawn, model call or durable
    /// external mutation belongs in this callback. Source-bearing admission
    /// remains held, and the operation is never exposed as a generic tool.
    pub fn with_vault_broker<R>(
        &self,
        actor: &PrincipalId,
        registry: &VaultProviderRegistryReader,
        authority: &impl VaultGrantAuthority,
        expected_grant_rev: u64,
        operation: impl FnOnce() -> Result<R>,
    ) -> Result<R> {
        self.with_vault_broker_at_epoch(
            actor,
            registry,
            authority,
            expected_grant_rev,
            None,
            operation,
        )
    }

    /// Host-selected broker also pins the original owning writer epoch.
    /// Check it in the same transaction as the actual provider operation.
    pub(crate) fn with_vault_broker_at_epoch<R>(
        &self,
        actor: &PrincipalId,
        registry: &VaultProviderRegistryReader,
        authority: &impl VaultGrantAuthority,
        expected_grant_rev: u64,
        writer_epoch: Option<u64>,
        operation: impl FnOnce() -> Result<R>,
    ) -> Result<R> {
        if self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "Vault broker requires the exclusive Store owner",
            ));
        }
        let policy = authority.policy();
        let target = &policy.target;
        if &target.actor != actor || target.projection != VaultProjection::Broker {
            return Err(Error::Denied(
                "Vault broker differs from authorized actor/projection",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = exact_key(&tx, &target.agent_ref)?;
        let key = AgentKey::new(&session, &branch);
        agent_owner::require_future_admission(&tx, key)?;
        if let Some(epoch) = writer_epoch {
            workspace_writer::verify_writer_epoch_for_session(&tx, &session, epoch)?;
        } else {
            workspace_writer::require_workspace_writer_for_session(&tx, &session)?;
        }
        require_projection_in(
            &tx,
            registry,
            authority,
            expected_grant_rev,
            VaultProjection::Broker,
        )?;
        let result = operation()?;
        db(tx.commit())?;
        Ok(result)
    }
}
