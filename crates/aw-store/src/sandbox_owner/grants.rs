//! Agent-to-Sandbox grant evidence. Host policy remains the authority;
//! these rows bind its exact decision to an Agent, actor and source set.
use super::{authorize_project, parse_sandbox_ref, source_bindings};
use crate::{
    agent_owner::{self, AgentKey, AgentPermission},
    audit_with_security_metadata, db, label, user_for_principal, Error, Result, SessionStore,
    VersionedSourceBinding,
};
use aw_agent_core::{
    validate_source_set_hard_limit, DriverKey, NamedSourceAccess, PrincipalId, SourceAccess,
    SourceName, SourceSetLimit,
};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_SOURCE_PINS_BYTES: usize = 6 * 1024 * 1024;

/// Non-secret references to the existing trusted host policy/authority.
/// Neither a persisted reference nor a matching digest is a live capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxPolicyEvidence {
    pub policy_ref: String,
    pub policy_digest: [u8; 32],
    pub authority_ref: String,
    pub authority_generation: u64,
}

impl SandboxPolicyEvidence {
    fn validate(&self) -> Result<()> {
        for reference in [&self.policy_ref, &self.authority_ref] {
            label(reference, 512)?;
            if reference.contains("://") || reference.chars().any(char::is_whitespace) {
                return Err(Error::Config(
                    "invalid non-secret Sandbox authority reference".into(),
                ));
            }
        }
        if self.authority_generation == 0 || self.authority_generation > i64::MAX as u64 {
            return Err(Error::Config("invalid Sandbox authority generation".into()));
        }
        Ok(())
    }
}

/// Trusted host decision for one exact actor. A group-backed host policy
/// may authorize that actor, but group/ACL changes never add actors here.
/// This is not a model-visible grant request or a physical mount policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxGrantPolicy {
    pub agent_ref: String,
    pub sandbox_ref: String,
    pub evidence: SandboxPolicyEvidence,
    pub actor: PrincipalId,
    pub actor_user_id: String,
    pub sources: Vec<SandboxSourceGrant>,
}

impl SandboxGrantPolicy {
    fn require_subject(
        &self,
        connection: &Connection,
        agent_ref: &str,
        sandbox: &str,
        actor: &PrincipalId,
    ) -> Result<()> {
        self.evidence.validate()?;
        crate::stable_user_principal(&self.actor_user_id)?;
        if self.agent_ref != agent_ref
            || self.sandbox_ref != sandbox
            || &self.actor != actor
            || user_for_principal(connection, actor)?.as_deref() != Some(&self.actor_user_id)
        {
            return Err(Error::Denied("Sandbox host authority subject changed"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxGrantStatus {
    /// Zero denotes no previous grant. Revocation retains a nonzero revision.
    pub revision: u64,
    pub active: bool,
}

/// Exact SourceBinding maximum remains in the Sandbox owner; the effective
/// access below is the intersection with the independent ceilings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveSourceBinding {
    pub binding: VersionedSourceBinding,
    pub access: SourceAccess,
}

/// Read-side preflight, not an execution capability. The host must hold
/// its live authority through effect admission and revalidate this boundary
/// in the owning transaction. Source-bearing runtime admission stays closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxGrantObservation {
    pub agent_ref: String,
    pub sandbox_ref: String,
    pub grant_revision: u64,
    pub actor: PrincipalId,
    pub access_generation: [u8; 32],
    pub policy: SandboxPolicyEvidence,
    pub sources: Vec<EffectiveSourceBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSourceGrant {
    pub name: SourceName,
    pub driver: DriverKey,
    pub source_ref: String,
    pub binding_rev: u64,
    pub access: SourceAccess,
}

impl SandboxSourceGrant {
    /// Pin the host's source-specific authorization to the exact observed
    /// binding, before any later Store transaction can select by name.
    pub fn from_binding(binding: &VersionedSourceBinding, access: SourceAccess) -> Self {
        Self {
            name: binding.binding.name.clone(),
            driver: binding.binding.driver.clone(),
            source_ref: binding.binding.source_ref.clone(),
            binding_rev: binding.binding_rev,
            access,
        }
    }

    fn validate(&self) -> Result<()> {
        self.name.validate()?;
        self.driver.validate()?;
        label(&self.source_ref, 2048)?;
        if self.source_ref.contains("://") || self.binding_rev == 0 {
            return Err(Error::Storage("invalid Sandbox grant source pin"));
        }
        Ok(())
    }

    fn matches(&self, binding: &VersionedSourceBinding) -> bool {
        self.name == binding.binding.name
            && self.driver == binding.binding.driver
            && self.source_ref == binding.binding.source_ref
            && self.binding_rev == binding.binding_rev
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Grant {
    status: SandboxGrantStatus,
    policy: SandboxPolicyEvidence,
    actor: PrincipalId,
    actor_user_id: String,
    access_generation: [u8; 32],
    sources: Vec<SandboxSourceGrant>,
}

impl Grant {
    fn validate(&self) -> Result<()> {
        self.policy.validate()?;
        crate::stable_user_principal(&self.actor_user_id)?;
        validate_source_set_hard_limit(self.sources.len())?;
        for pin in &self.sources {
            pin.validate()?;
        }
        if self
            .sources
            .windows(2)
            .any(|pair| pair[0].name.as_str() >= pair[1].name.as_str())
            || self.status.revision == 0
        {
            return Err(Error::Storage("invalid Sandbox grant ordering or revision"));
        }
        Ok(())
    }
}

fn grant_in(connection: &Connection, key: AgentKey<'_>, sandbox: &str) -> Result<Option<Grant>> {
    let mut query = db(connection.prepare_cached(
        "SELECT grant_rev,state,policy_ref,policy_digest,authority_ref,authority_generation,
                actor_principal,actor_user_id,access_generation,source_pins_json
         FROM agent_sandbox_grants_current
         WHERE session_id=?1 AND branch_id=?2 AND sandbox_id=?3",
    ))?;
    let mut rows = db(query.query(params![key.session, key.branch, sandbox]))?;
    let Some(row) = db(rows.next())? else {
        return Ok(None);
    };
    let state: String = db(row.get(1))?;
    let active = match state.as_str() {
        "active" => true,
        "revoked" => false,
        _ => return Err(Error::Storage("invalid Sandbox grant state")),
    };
    let pins = db(row.get_ref(9))?
        .as_str()
        .map_err(|_| Error::Storage("invalid Sandbox grant source pins"))?;
    if pins.len() > MAX_SOURCE_PINS_BYTES {
        return Err(Error::Limit(
            "Sandbox grant source pins exceed durable bound",
        ));
    }
    let grant = Grant {
        status: SandboxGrantStatus {
            revision: db(row.get(0))?,
            active,
        },
        policy: SandboxPolicyEvidence {
            policy_ref: db(row.get(2))?,
            policy_digest: db(row.get(3))?,
            authority_ref: db(row.get(4))?,
            authority_generation: db(row.get(5))?,
        },
        actor: PrincipalId::new(db(row.get::<_, String>(6))?)?,
        actor_user_id: db(row.get(7))?,
        access_generation: db(row.get(8))?,
        sources: crate::decode(pins)?,
    };
    grant.validate()?;
    Ok(Some(grant))
}

fn binding_in(connection: &Connection, key: AgentKey<'_>) -> Result<String> {
    let sandbox: Option<String> = db(connection.query_row(
        "SELECT sandbox_id FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![key.session, key.branch],
        |row| row.get(0),
    ))?;
    sandbox.ok_or(Error::Denied("Agent has no authoritative Sandbox binding"))
}

fn require_open_binding_in(
    connection: &Connection,
    key: AgentKey<'_>,
    sandbox: &str,
) -> Result<()> {
    let open: bool = db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM agent_state_current a
             JOIN sandboxes_current s ON s.sandbox_id=a.sandbox_id
             JOIN workspaces w ON w.workspace_id=a.project_handle AND w.domain_id=a.domain_handle
             WHERE a.session_id=?1 AND a.branch_id=?2 AND s.sandbox_id=?3
               AND a.lifecycle='open' AND s.lifecycle='open' AND w.archived_at IS NULL
               AND a.domain_handle=s.domain_handle AND a.identity_handle=s.identity_handle
               AND a.project_handle=s.project_handle
         )",
        params![key.session, key.branch, sandbox],
        |row| row.get(0),
    ))?;
    if !open {
        return Err(Error::Denied(
            "Agent Sandbox binding is not open in its exact Project",
        ));
    }
    Ok(())
}

fn ceilings(
    entries: &[NamedSourceAccess],
    limit: SourceSetLimit,
) -> Result<BTreeMap<&str, SourceAccess>> {
    limit.admit(entries.len())?;
    let mut map = BTreeMap::new();
    for entry in entries {
        entry.name.validate()?;
        if map.insert(entry.name.as_str(), entry.access).is_some() {
            return Err(Error::Conflict("duplicate named source ceiling"));
        }
    }
    Ok(map)
}

fn selected_bindings(
    store: &SessionStore,
    connection: &Connection,
    sandbox: &str,
    names: &[SourceName],
) -> Result<Vec<VersionedSourceBinding>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    source_bindings::select_in(
        connection,
        sandbox,
        Some(names),
        store.inner.source_limit,
        &store.inner.source_drivers.reader()?,
    )
}

fn grant_audit(
    tx: &Connection,
    actor: &PrincipalId,
    agent: AgentKey<'_>,
    status: SandboxGrantStatus,
) -> Result<()> {
    // No policy/source locators or privileged material in audit presentation.
    let data = serde_json::json!({"branch_id": agent.branch, "grant_rev": status.revision, "active":status.active});
    audit_with_security_metadata(
        tx,
        agent.session,
        actor,
        "sandbox.grant.changed",
        data,
        None,
    )
}

impl SessionStore {
    /// Trusted host input to a new independent source approval, not a
    /// grant or model-facing metadata API. The caller must independently
    /// authorize the exact selected sources and metadata before use.
    /// No current SandboxGrant is required or consulted; its eventual
    /// publication still revalidates every binding pin and actor.
    pub fn sandbox_grant_sources(
        &self,
        manager: &PrincipalId,
        actor: &PrincipalId,
        agent_ref: &str,
        expected_sandbox_rev: u64,
        names: &[SourceName],
    ) -> Result<(super::SandboxSnapshot, Vec<VersionedSourceBinding>)> {
        label(agent_ref, 2048)?;
        if names.is_empty() {
            return Err(Error::Denied("source approval requires a named selection"));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let (session, branch) = agent_owner::key_for_ref(&tx, agent_ref)?;
        let key = AgentKey::new(&session, &branch);
        let sandbox = binding_in(&tx, key)?;
        authorize_project(&tx, manager, &parse_sandbox_ref(&sandbox)?.project, true)?;
        agent_owner::authorize_permission(&tx, actor, key, AgentPermission::Submit)?;
        require_open_binding_in(&tx, key, &sandbox)?;
        if agent_owner::canonical_ref(&tx, key)? != agent_ref {
            return Err(Error::Denied(
                "source approval requires the exact Agent reference",
            ));
        }
        let current = super::snapshot_with_sources(&tx, &sandbox)?;
        if current.rev != expected_sandbox_rev {
            return Err(Error::Conflict("Sandbox changed before source approval"));
        }
        let sources = selected_bindings(self, &tx, &sandbox, names)?;
        db(tx.commit())?;
        Ok((current, sources))
    }

    /// Trusted host only: the caller must independently authorize the
    /// policy, exact actor and every source ceiling. Project creation/manage
    /// authority alone cannot manufacture that policy. None revokes, and
    /// expected_revision=0 creates only when no previous grant exists.
    /// Does not alter Agent ACL/rev, Sandbox rev, admission or running work.
    pub fn sandbox_grant_set(
        &self,
        manager: &PrincipalId,
        agent_ref: &str,
        expected_revision: u64,
        policy: Option<&SandboxGrantPolicy>,
    ) -> Result<SandboxGrantStatus> {
        label(agent_ref, 2048)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = agent_owner::key_for_ref(&tx, agent_ref)?;
        let key = AgentKey::new(&session, &branch);
        let sandbox = binding_in(&tx, key)?;
        let project = parse_sandbox_ref(&sandbox)?.project;
        authorize_project(&tx, manager, &project, true)?;
        let current = grant_in(&tx, key, &sandbox)?;
        let before = current.as_ref().map_or(
            SandboxGrantStatus {
                revision: 0,
                active: false,
            },
            |grant| grant.status,
        );
        if before.revision != expected_revision {
            return Err(Error::Conflict("stale Sandbox grant revision"));
        }
        let next_revision = || {
            before
                .revision
                .checked_add(1)
                .filter(|rev| *rev <= i64::MAX as u64)
                .ok_or(Error::Limit("Sandbox grant revision exhausted"))
        };
        let status = if let Some(policy) = policy {
            require_open_binding_in(&tx, key, &sandbox)?;
            policy.require_subject(&tx, agent_ref, &sandbox, &policy.actor)?;
            let access = agent_owner::authorize_permission(
                &tx,
                &policy.actor,
                key,
                AgentPermission::Submit,
            )?;
            self.inner.source_limit.admit(policy.sources.len())?;
            let mut sources = policy.sources.clone();
            sources.sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
            for source in &sources {
                source.validate()?;
            }
            let names = sources
                .iter()
                .map(|source| source.name.clone())
                .collect::<Vec<_>>();
            let bindings = selected_bindings(self, &tx, &sandbox, &names)?;
            for (pin, version) in sources.iter_mut().zip(bindings) {
                if !pin.matches(&version) {
                    return Err(Error::Conflict(
                        "host source authorization refers to another binding revision",
                    ));
                }
                pin.access = pin.access.intersect(version.binding.access);
            }
            let mut next = Grant {
                status: SandboxGrantStatus {
                    revision: before.revision.max(1),
                    active: true,
                },
                policy: policy.evidence.clone(),
                actor: policy.actor.clone(),
                actor_user_id: policy.actor_user_id.clone(),
                access_generation: access.access_generation,
                sources,
            };
            next.validate()?;
            if current.as_ref() == Some(&next) {
                return Ok(before);
            }
            next.status.revision = next_revision()?;
            let pins = crate::json(&next.sources)?;
            if pins.len() > MAX_SOURCE_PINS_BYTES {
                return Err(Error::Limit(
                    "Sandbox grant source pins exceed durable bound",
                ));
            }
            db(tx.execute(
                "INSERT INTO agent_sandbox_grants_current(
                    session_id,branch_id,sandbox_id,grant_rev,state,policy_ref,policy_digest,
                    authority_ref,authority_generation,actor_principal,actor_user_id,
                    access_generation,source_pins_json
                 ) VALUES(?1,?2,?3,?4,'active',?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(session_id,branch_id,sandbox_id) DO UPDATE SET
                    grant_rev=excluded.grant_rev,state='active',policy_ref=excluded.policy_ref,
                    policy_digest=excluded.policy_digest,authority_ref=excluded.authority_ref,
                    authority_generation=excluded.authority_generation,actor_principal=excluded.actor_principal,
                    actor_user_id=excluded.actor_user_id,access_generation=excluded.access_generation,
                    source_pins_json=excluded.source_pins_json",
                params![session,branch,sandbox,next.status.revision,next.policy.policy_ref,
                    next.policy.policy_digest,next.policy.authority_ref,next.policy.authority_generation,
                    next.actor.as_str(),next.actor_user_id,next.access_generation,pins],
            ))?;
            next.status
        } else {
            if !before.active {
                return Ok(before);
            }
            let revision = next_revision()?;
            db(tx.execute(
                "UPDATE agent_sandbox_grants_current SET grant_rev=?1,state='revoked'
                 WHERE session_id=?2 AND branch_id=?3 AND sandbox_id=?4",
                params![revision, session, branch, sandbox],
            ))?;
            SandboxGrantStatus {
                revision,
                active: false,
            }
        };
        grant_audit(&tx, manager, key, status)?;
        db(tx.commit())?;
        Ok(status)
    }

    /// Project-authorized host status, with no protected policy/source data.
    pub fn sandbox_grant_status(
        &self,
        actor: &PrincipalId,
        agent_ref: &str,
    ) -> Result<SandboxGrantStatus> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let (session, branch) = agent_owner::key_for_ref(&tx, agent_ref)?;
        let key = AgentKey::new(&session, &branch);
        let sandbox = binding_in(&tx, key)?;
        authorize_project(&tx, actor, &parse_sandbox_ref(&sandbox)?.project, false)?;
        let status = grant_in(&tx, key, &sandbox)?.map_or(
            SandboxGrantStatus {
                revision: 0,
                active: false,
            },
            |grant| grant.status,
        );
        db(tx.commit())?;
        Ok(status)
    }

    /// Trusted host preflight. current_policy must come from the live policy
    /// owner, not from a stored grant or model. Tool ceilings are supplied
    /// by the current tool authority. Omitted names grant no access.
    /// This observation never opens held admission or permits execution.
    pub fn sandbox_grant_preflight(
        &self,
        actor: &PrincipalId,
        agent_ref: &str,
        current_policy: &SandboxGrantPolicy,
        tool_ceiling: &[NamedSourceAccess],
    ) -> Result<SandboxGrantObservation> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let observation =
            self.sandbox_grant_preflight_in(&tx, actor, agent_ref, current_policy, tool_ceiling)?;
        db(tx.commit())?;
        Ok(observation)
    }

    /// Shared read-side boundary for a future effect admission transaction.
    /// The caller separately owns the live host policy fence.
    pub(crate) fn sandbox_grant_preflight_in(
        &self,
        connection: &Connection,
        actor: &PrincipalId,
        agent_ref: &str,
        current_policy: &SandboxGrantPolicy,
        tool_ceiling: &[NamedSourceAccess],
    ) -> Result<SandboxGrantObservation> {
        let (session, branch) = agent_owner::key_for_ref(connection, agent_ref)?;
        let key = AgentKey::new(&session, &branch);
        let sandbox = binding_in(connection, key)?;
        require_open_binding_in(connection, key, &sandbox)?;
        let access =
            agent_owner::authorize_permission(connection, actor, key, AgentPermission::Submit)?;
        let grant = grant_in(connection, key, &sandbox)?
            .ok_or(Error::Denied("independent Sandbox grant is unavailable"))?;
        current_policy.require_subject(connection, agent_ref, &sandbox, actor)?;
        if !grant.status.active
            || &grant.actor != actor
            || grant.actor_user_id != current_policy.actor_user_id
            || grant.access_generation != access.access_generation
            || grant.policy != current_policy.evidence
        {
            return Err(Error::Denied(
                "Sandbox grant actor or current authority changed",
            ));
        }
        self.inner
            .source_limit
            .admit(current_policy.sources.len())?;
        let mut policy = BTreeMap::new();
        for source in &current_policy.sources {
            source.validate()?;
            if policy.insert(source.name.as_str(), source).is_some() {
                return Err(Error::Conflict("duplicate named host source ceiling"));
            }
        }
        let tools = ceilings(tool_ceiling, self.inner.source_limit)?;
        let names = grant
            .sources
            .iter()
            .map(|source| source.name.clone())
            .collect::<Vec<_>>();
        let bindings = selected_bindings(self, connection, &sandbox, &names)?;
        let mut sources = Vec::with_capacity(bindings.len());
        for (pin, binding) in grant.sources.iter().zip(bindings) {
            if !pin.matches(&binding) {
                return Err(Error::Conflict("Sandbox grant source binding changed"));
            }
            let Some(policy) = policy.get(pin.name.as_str()) else {
                continue;
            };
            if !policy.matches(&binding) {
                return Err(Error::Conflict(
                    "current host source authorization refers to another binding revision",
                ));
            }
            let Some(tool) = tools.get(pin.name.as_str()) else {
                continue;
            };
            let access = binding
                .binding
                .access
                .intersect(pin.access)
                .intersect(policy.access)
                .intersect(*tool);
            sources.push(EffectiveSourceBinding { binding, access });
        }
        Ok(SandboxGrantObservation {
            agent_ref: agent_ref.to_owned(),
            sandbox_ref: sandbox,
            grant_revision: grant.status.revision,
            actor: actor.clone(),
            access_generation: access.access_generation,
            policy: grant.policy,
            sources,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentAdmission, AgentPermissions, SessionBinding};
    use aw_agent_core::{
        CheckpointPolicy, SchemaSupport, SourceBinding, SourceDriverRegistry, SourceDriverSchemas,
    };
    use std::path::PathBuf;

    struct PrivateTemp(PathBuf);
    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sandbox_grants_pin_actor_authority_and_exact_sources_across_restart() {
        let temp = PrivateTemp(std::env::temp_dir().join(crate::id("aw-sandbox-grant").unwrap()));
        let owner = PrincipalId::new("internal:fixture:grant-owner").unwrap();
        let participant = PrincipalId::new("internal:fixture:grant-participant").unwrap();
        let store = SessionStore::open(&temp.0).unwrap();
        let layout = SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let (session, branch) = store.create(&owner, "first", layout.clone()).unwrap();
        let (other, other_branch) = store.create(&owner, "second", layout).unwrap();
        let connection = store.connection().unwrap();
        let key = AgentKey::new(&session.id, &branch.id);
        let other_key = AgentKey::new(&other.id, &other_branch.id);
        let reference = agent_owner::canonical_ref(&connection, key).unwrap();
        let other_ref = agent_owner::canonical_ref(&connection, other_key).unwrap();
        let sandbox = binding_in(&connection, key).unwrap();
        assert_eq!(binding_in(&connection, other_key).unwrap(), sandbox);
        drop(connection);
        drop(store);

        // Populated v57 migration; no implicit grants are seeded.
        {
            let connection = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            connection
                .execute_batch(
                    "DROP TABLE agent_vault_grants_current;
                 DROP TABLE agent_sandbox_grants_current;
                 UPDATE store_meta SET value='57' WHERE key='schema_epoch';
                 UPDATE schema_meta SET schema_epoch=57,activation_epoch=57 WHERE singleton=1;
                 UPDATE migration_lease SET current_epoch=57 WHERE singleton=1;
                 PRAGMA user_version=57;",
                )
                .unwrap();
        }
        let registry = SourceDriverRegistry::new();
        let driver = DriverKey::builtin("fixture-grant").unwrap();
        let schema = SchemaSupport::new(1, 1).unwrap();
        registry
            .register(
                driver.clone(),
                SourceDriverSchemas {
                    binding: schema,
                    anchor: schema,
                    recovery: schema,
                },
                (),
            )
            .unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        let no_grant = SandboxGrantStatus {
            revision: 0,
            active: false,
        };
        assert_eq!(
            store.sandbox_grant_status(&owner, &reference).unwrap(),
            no_grant
        );
        assert_eq!(
            store.sandbox_grant_status(&owner, &other_ref).unwrap(),
            no_grant
        );
        let sources = ["code", "data"].map(|name| SourceBinding {
            name: SourceName::new(name).unwrap(),
            driver: driver.clone(),
            binding_schema: 1,
            source_ref: format!("fixture-source:{name}"),
            materialization: format!("fixture-materialization:{name}"),
            access: if name == "code" {
                SourceAccess::ReadWrite
            } else {
                SourceAccess::ReadOnly
            },
            checkpoint_policy: CheckpointPolicy::Required,
        });
        // A first binding can attach to an independently prepared unused
        // layout, but cannot retrofit the existing ambient Agent layout.
        assert!(store
            .sandbox_sources_set(&owner, &sandbox, &sources[0].name, Some(&sources[0]), 1,)
            .is_err());
        let unused_layout = SessionBinding {
            working_directory: temp.0.join("unused"),
            tool_root: temp.0.join("unused"),
            system_prompt: "fixture".into(),
            profile_hint: "fixture".into(),
            model_hint: "fixture".into(),
            destination_fingerprint: "0".repeat(64),
        };
        let project = store.agent_get(&owner, &reference).unwrap().project_ref;
        let unused = store
            .sandbox_create_fresh(&owner, &project, &unused_layout)
            .unwrap();
        let attached = store
            .sandbox_sources_set(
                &owner,
                &unused.canonical_ref,
                &sources[0].name,
                Some(&sources[0]),
                unused.rev,
            )
            .unwrap();
        assert_eq!(attached.rev, unused.rev + 1);
        assert_eq!(attached.sources[0].binding_rev, attached.rev);
        // Test-only structural association: both Agents share a real
        // persisted Sandbox but are held. Production attach/realization is
        // still closed; this fixture does not add a rebind API.
        {
            let mut connection = store.connection().unwrap();
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            assert_eq!(
                tx.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                crate::LATEST_SCHEMA_VERSION
            );
            source_bindings::insert(&tx, &sandbox, &sources).unwrap();
            tx.execute(
                "UPDATE agent_state_current SET admission='held' WHERE sandbox_id=?1",
                [&sandbox],
            )
            .unwrap();
            tx.commit().unwrap();
        }
        let before = store.agent_get(&owner, &reference).unwrap();
        let before_sandbox = store.sandbox_get(&owner, &sandbox).unwrap();
        // Protected inputs for a new independent approval come from the
        // original owner, not from an already-published grant. Selection
        // is exact and canonical, never widened to a whole layout.
        let names = [sources[1].name.clone(), sources[0].name.clone()];
        let (layout, selected) = store
            .sandbox_grant_sources(&owner, &owner, &reference, before_sandbox.rev, &names)
            .unwrap();
        assert_eq!(layout, before_sandbox);
        assert_eq!(
            selected,
            sources
                .iter()
                .cloned()
                .map(|binding| VersionedSourceBinding {
                    binding,
                    binding_rev: 1,
                })
                .collect::<Vec<_>>()
        );
        assert!(store
            .sandbox_grant_sources(&participant, &owner, &reference, before_sandbox.rev, &names,)
            .is_err());
        assert!(store
            .sandbox_grant_sources(&owner, &owner, &reference, before_sandbox.rev + 1, &names,)
            .is_err());
        assert!(store
            .sandbox_grant_sources(
                &owner,
                &owner,
                &reference,
                before_sandbox.rev,
                &[names[0].clone(), names[0].clone()],
            )
            .is_err());
        assert!(
            !store
                .sandbox_grant_status(&owner, &reference)
                .unwrap()
                .active
        );
        let mut policy = SandboxGrantPolicy {
            agent_ref: reference.clone(),
            sandbox_ref: sandbox.clone(),
            actor_user_id: user_for_principal(&store.connection().unwrap(), &owner)
                .unwrap()
                .unwrap(),
            evidence: SandboxPolicyEvidence {
                policy_ref: "host:fixture-policy".into(),
                policy_digest: [7; 32],
                authority_ref: "host:fixture-authority".into(),
                authority_generation: 1,
            },
            actor: owner.clone(),
            sources: sources
                .iter()
                .cloned()
                .map(|binding| {
                    SandboxSourceGrant::from_binding(
                        &VersionedSourceBinding {
                            binding,
                            binding_rev: 1,
                        },
                        SourceAccess::ReadWrite,
                    )
                })
                .collect(),
        };
        // A source name alone is not a grant. Issuance itself must pin
        // the host's exact source observation, and metadata changes do not
        // move Agent/Sandbox revisions.
        let tool_rw = sources
            .iter()
            .map(|source| NamedSourceAccess {
                name: source.name.clone(),
                access: SourceAccess::ReadWrite,
            })
            .collect::<Vec<_>>();
        assert!(store
            .sandbox_grant_preflight(&owner, &reference, &policy, &tool_rw)
            .is_err());
        let first = store
            .sandbox_grant_set(&owner, &reference, 0, Some(&policy))
            .unwrap();
        assert_eq!(
            first,
            SandboxGrantStatus {
                revision: 1,
                active: true
            }
        );
        assert_eq!(
            store
                .sandbox_grant_set(&owner, &reference, 1, Some(&policy))
                .unwrap(),
            first
        );
        assert!(store
            .sandbox_grant_set(&owner, &other_ref, 0, Some(&policy))
            .is_err());
        assert_eq!(store.agent_get(&owner, &reference).unwrap(), before);
        assert_eq!(store.sandbox_get(&owner, &sandbox).unwrap(), before_sandbox);
        let observation = store
            .sandbox_grant_preflight(&owner, &reference, &policy, &tool_rw)
            .unwrap();
        assert_eq!(observation.sources.len(), 2);
        assert!(observation.sources[0]
            .access
            .permits(SourceAccess::ReadWrite));
        assert!(!observation.sources[1]
            .access
            .permits(SourceAccess::ReadWrite));
        assert!(store
            .sandbox_grant_preflight(&owner, &other_ref, &policy, &tool_rw)
            .is_err());
        let mut narrower = policy.clone();
        narrower.sources[0].access = SourceAccess::ReadOnly;
        assert_eq!(
            store
                .sandbox_grant_preflight(&owner, &reference, &narrower, &tool_rw)
                .unwrap()
                .sources[0]
                .access,
            SourceAccess::ReadOnly,
        );
        assert_eq!(
            store
                .sandbox_grant_preflight(
                    &owner,
                    &reference,
                    &policy,
                    &[NamedSourceAccess {
                        name: sources[0].name.clone(),
                        access: SourceAccess::ReadOnly,
                    }]
                )
                .unwrap()
                .sources
                .iter()
                .map(|source| source.access)
                .collect::<Vec<_>>(),
            [SourceAccess::ReadOnly],
        );
        narrower.evidence.authority_generation += 1;
        assert!(store
            .sandbox_grant_preflight(&owner, &reference, &narrower, &tool_rw)
            .is_err());

        // Later ACL broadening neither inherits another actor's grant nor
        // preserves the old access-generation dependency.
        let domain = store.default_domain(&owner).unwrap();
        let participant_ref = {
            let connection = store.connection().unwrap();
            let user = crate::ensure_user_for_principal(&connection, &participant).unwrap();
            format!("/d:{}/i:{user}", domain.id)
        };
        store
            .agent_access_set(
                &owner,
                &session.id,
                &branch.id,
                &participant_ref,
                Some(AgentPermissions {
                    view: true,
                    submit: true,
                    manage: true,
                    ..Default::default()
                }),
                before.rev,
            )
            .unwrap();
        assert!(store
            .sandbox_grant_preflight(&owner, &reference, &policy, &tool_rw)
            .is_err());
        policy.actor = participant.clone();
        policy.actor_user_id = participant_ref.rsplit_once("/i:").unwrap().1.to_owned();
        assert!(store
            .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
            .is_err());
        assert!(store
            .sandbox_grant_set(&participant, &reference, 1, Some(&policy))
            .is_err());
        assert_eq!(
            store
                .sandbox_grant_set(&owner, &reference, 1, Some(&policy))
                .unwrap()
                .revision,
            2
        );
        let observation = store
            .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
            .unwrap();
        assert_eq!(observation.grant_revision, 2);

        // Real metadata mutation owner: CAS, independent Project authority,
        // complete old/new barrier coverage, and no resurrection of a grant
        // when an identical source is removed and later reattached.
        let before_agent = store.agent_get(&owner, &reference).unwrap();
        let before_other = store.agent_get(&owner, &other_ref).unwrap();
        let current = store.sandbox_get(&owner, &sandbox).unwrap();
        let mut replacement = sources[0].clone();
        replacement.source_ref = "fixture-source:replacement".into();
        let set = |actor: &PrincipalId, binding: Option<&SourceBinding>, rev| {
            store.sandbox_sources_set(actor, &sandbox, &sources[0].name, binding, rev)
        };
        assert!(set(&participant, Some(&replacement), current.rev).is_err());
        let old_other = crate::SourceMutationSet::from_bindings(&sources[1..]).unwrap();
        let incoming =
            crate::SourceMutationSet::from_bindings(std::slice::from_ref(&replacement)).unwrap();
        for (claim, id) in [
            (&old_other, "host:old-layout"),
            (&incoming, "host:incoming"),
        ] {
            let barrier = store.acquire_source_mutation_barrier(claim, id).unwrap();
            assert!(set(&owner, Some(&replacement), current.rev).is_err());
            assert_eq!(store.sandbox_get(&owner, &sandbox).unwrap(), current);
            barrier
                .settle(crate::SourceMutationOutcome::Settled)
                .unwrap();
        }
        let mut extra = sources[0].clone();
        extra.name = SourceName::new("extra").unwrap();
        assert!(matches!(
            store.sandbox_sources_set(&owner, &sandbox, &extra.name, Some(&extra), current.rev),
            Err(Error::Limit(_))
        ));

        let removed = set(&owner, None, current.rev).unwrap();
        assert_eq!(removed.rev, current.rev + 1);
        assert_eq!(removed.sources.len(), 1);
        assert!(store
            .sandbox_sources_set(&owner, &sandbox, &sources[1].name, None, removed.rev,)
            .is_err());
        assert!(store
            .sandbox_sources_detach_final(
                &owner,
                &sandbox,
                &sources[1].name,
                removed.rev,
                |_, _| -> Result<()> {
                    panic!("an existing Agent binding cannot become an ambient layout")
                }
            )
            .is_err());
        let reattached = set(&owner, Some(&sources[0]), removed.rev).unwrap();
        assert_eq!(reattached.sources[0].binding_rev, reattached.rev);
        assert!(store
            .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
            .is_err());
        let changed = set(&owner, Some(&replacement), reattached.rev).unwrap();
        assert_eq!(changed.rev, reattached.rev + 1);
        assert_eq!(changed.sources[0].binding_rev, changed.rev);
        assert_eq!(changed.sources[1].binding_rev, 1);
        assert!(set(&owner, Some(&replacement), current.rev).is_err());
        assert_eq!(
            set(&owner, Some(&replacement), changed.rev).unwrap(),
            changed
        );
        assert_eq!(store.agent_get(&owner, &reference).unwrap(), before_agent);
        assert_eq!(store.agent_get(&owner, &other_ref).unwrap(), before_other);
        assert!(store
            .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
            .is_err());
        assert!(store
            .sandbox_grant_set(&owner, &reference, 2, Some(&policy))
            .is_err());
        policy.sources[0].binding_rev = changed.rev;
        policy.sources[0].source_ref = replacement.source_ref;
        policy.sources[0].access = SourceAccess::ReadOnly;
        assert_eq!(
            store
                .sandbox_grant_set(&owner, &reference, 2, Some(&policy))
                .unwrap()
                .revision,
            3
        );
        let observation = store
            .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
            .unwrap();
        assert_eq!(observation.sources[0].access, SourceAccess::ReadOnly);
        assert!(store.activate(&owner, &session.id, &branch.id).is_err());
        assert!(store
            .agent_admission(&owner, &reference, AgentAdmission::Open, None)
            .is_err());
        drop(store);

        // Durable grants do not authorize incompatible/missing drivers.
        // Revocation works independently of source compatibility.
        let store = SessionStore::open(&temp.0).unwrap();
        assert_eq!(
            store
                .sandbox_grant_status(&owner, &reference)
                .unwrap()
                .revision,
            3
        );
        assert!(store
            .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
            .is_err());
        let revoked = store
            .sandbox_grant_set(&owner, &reference, 3, None)
            .unwrap();
        assert_eq!(
            revoked,
            SandboxGrantStatus {
                revision: 4,
                active: false
            }
        );
        assert!(store
            .sandbox_grant_set(&owner, &reference, 3, Some(&policy))
            .is_err());
        drop(store);

        let store = SessionStore::open_with_sources(&temp.0, registry, limit).unwrap();
        assert_eq!(
            store.sandbox_grant_status(&owner, &reference).unwrap(),
            revoked
        );
        assert!(store
            .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
            .is_err());
        assert_eq!(
            store
                .sandbox_grant_set(&owner, &reference, 4, Some(&policy))
                .unwrap()
                .revision,
            5
        );
        assert_eq!(
            store
                .sandbox_grant_preflight(&participant, &reference, &policy, &tool_rw)
                .unwrap()
                .grant_revision,
            5
        );
        assert_eq!(
            store.sandbox_grant_status(&owner, &other_ref).unwrap(),
            no_grant
        );
    }
}
