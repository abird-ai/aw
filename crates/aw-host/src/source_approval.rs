//! One independently approved native source directory. Own the actual
//! physical pin and live policy decision, not a registry of copied grants.
use aw_agent_core::{
    AgentExecutionBinding, Error, NamedSourceAccess, PrincipalId, Result, SourceAccess,
};
use aw_store::{
    AgentLifecycle, SandboxEffectAuthority, SandboxGrantObservation, SandboxGrantPolicy,
    SandboxGrantStatus, SandboxPolicyEvidence, SandboxSourceGrant, SessionStore,
    VersionedSourceBinding,
};
use aw_tools_adapter::native_filesystem::{
    CapabilitySet, GrantIdentity, GrantSpec, PreparedGrant, PrivateArtifactRegistry,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::{OwnedRwLockReadGuard, RwLock};

/// Explicit trusted-host approval. None of these inputs are model arguments
/// or reconstructed from a persisted SandboxGrant. The host owns the stable
/// authority identity/generation and must not reuse a revoked generation.
pub struct NativeSourceApprovalSpec {
    pub agent_ref: String,
    pub actor: PrincipalId,
    pub policy_ref: String,
    pub authority_ref: String,
    pub authority_generation: u64,
    pub source: VersionedSourceBinding,
    pub directory: GrantSpec,
}

struct Decision {
    agent: AgentExecutionBinding,
    policy: SandboxGrantPolicy,
    source: VersionedSourceBinding,
    physical: GrantIdentity,
    tool_ceiling: [NamedSourceAccess; 1],
    private_artifacts: Arc<PrivateArtifactRegistry>,
    // Retain the exact approved directory object, independent of its name.
    // Never use this maximum capability directly for a tool effect.
    _pin: CapabilitySet,
}

/// Live host policy for one exact Agent and one source. Revocation is
/// permanent for this owner. New approval requires a new host decision.
/// Store grant changes remain with the existing SandboxGrant owner.
#[derive(Clone)]
pub struct NativeSourceApproval {
    store: SessionStore,
    current: Arc<RwLock<Option<Decision>>>,
}

pub struct NativeSourceApprovalGuard(OwnedRwLockReadGuard<Option<Decision>>);

impl NativeSourceApprovalGuard {
    fn decision(&self) -> &Decision {
        // Only read() constructs this guard, after checking Some.
        self.0.as_ref().expect("live source approval guard")
    }

    pub fn agent(&self) -> &AgentExecutionBinding {
        &self.decision().agent
    }

    pub fn source(&self) -> &VersionedSourceBinding {
        &self.decision().source
    }

    pub fn physical(&self) -> &GrantIdentity {
        &self.decision().physical
    }

    pub fn private_artifacts(&self) -> Arc<PrivateArtifactRegistry> {
        self.decision().private_artifacts.clone()
    }
}

impl SandboxEffectAuthority for NativeSourceApprovalGuard {
    fn policy(&self) -> &SandboxGrantPolicy {
        &self.decision().policy
    }

    fn tool_ceiling(&self) -> &[NamedSourceAccess] {
        &self.decision().tool_ceiling
    }
}

impl NativeSourceApproval {
    /// Pin only an independently approved, exact absolute directory. This
    /// neither materializes a source nor reconciles private artifacts. The
    /// original host supplies the artifact owner for that directory.
    pub fn pin(
        store: SessionStore,
        spec: NativeSourceApprovalSpec,
        private_artifacts: Arc<PrivateArtifactRegistry>,
    ) -> Result<Self> {
        spec.source.binding.validate()?;
        let access = if spec.directory.write {
            SourceAccess::ReadWrite
        } else {
            SourceAccess::ReadOnly
        };
        if spec.source.binding_rev == 0
            || !spec.directory.read
            || !spec.source.binding.access.permits(access)
            || !spec.directory.path.is_absolute()
            || spec.directory.path.parent().is_none()
            || spec.authority_generation == 0
            || spec.authority_generation > i64::MAX as u64
            || [&spec.policy_ref, &spec.authority_ref]
                .iter()
                .any(|reference| {
                    reference.is_empty()
                        || reference.len() > 512
                        || reference.contains("://")
                        || reference.chars().any(char::is_whitespace)
                        || reference.chars().any(char::is_control)
                })
        {
            return Err(Error::Denied("invalid independent native source approval"));
        }
        let agent = store.agent_get(&spec.actor, &spec.agent_ref)?;
        if agent.canonical_ref != spec.agent_ref
            || agent.lifecycle != AgentLifecycle::Open
            || !agent.caller_permissions.submit
        {
            return Err(Error::Denied(
                "native source approval requires its exact open Agent",
            ));
        }
        let sandbox_ref = agent
            .sandbox_ref
            .ok_or(Error::Denied("Agent has no Sandbox"))?;
        let actor_user_id = store.current_user(&spec.actor)?.user_id;
        let execution = store.agent_execution_binding(&spec.actor, &spec.agent_ref)?;
        let path = spec.directory.path.clone();
        let pin = CapabilitySet::open_prepared(
            &path,
            [PreparedGrant::new(
                spec.directory,
                private_artifacts.clone(),
            )],
        )
        .map_err(|_| {
            Error::Denied("independently approved native directory could not be pinned")
        })?;
        let mut identities = pin
            .identities()
            .map_err(|_| Error::Denied("approved native directory identity unavailable"))?;
        if identities.len() != 1 {
            return Err(Error::Denied(
                "native source approval requires one directory",
            ));
        }
        let physical = identities.pop().expect("one approved directory");
        if physical.path != path
            || pin.base() != path
            || !physical.read
            || physical.write != (access == SourceAccess::ReadWrite)
        {
            return Err(Error::Denied(
                "native source approval cannot follow directory aliases",
            ));
        }
        // The policy digest names the real native approval, not a digest
        // copied out of the durable SandboxGrant evidence.
        let encoded = serde_json::to_vec(&(
            &spec.source,
            access,
            &physical.path,
            physical.volume,
            physical.object,
        ))
        .map_err(|_| Error::Storage("native source approval identity cannot be encoded"))?;
        let mut digest = Sha256::new();
        digest.update(b"aw-native-source-approval-v1\0");
        digest.update(encoded);
        let policy = SandboxGrantPolicy {
            agent_ref: spec.agent_ref,
            sandbox_ref,
            actor: spec.actor,
            actor_user_id,
            evidence: SandboxPolicyEvidence {
                policy_ref: spec.policy_ref,
                policy_digest: digest.finalize().into(),
                authority_ref: spec.authority_ref,
                authority_generation: spec.authority_generation,
            },
            sources: vec![SandboxSourceGrant::from_binding(&spec.source, access)],
        };
        let tool_ceiling = [NamedSourceAccess {
            name: spec.source.binding.name.clone(),
            access,
        }];
        Ok(Self {
            store,
            current: Arc::new(RwLock::new(Some(Decision {
                agent: execution,
                policy,
                source: spec.source,
                physical,
                tool_ceiling,
                private_artifacts,
                _pin: pin,
            }))),
        })
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    pub fn read(&self) -> Result<NativeSourceApprovalGuard> {
        // Never block an async scheduler or wait behind pending revocation.
        let guard = self
            .current
            .clone()
            .try_read_owned()
            .map_err(|_| Error::Denied("native source approval is changing"))?;
        if guard.is_none() {
            return Err(Error::Denied("native source approval was revoked"));
        }
        Ok(NativeSourceApprovalGuard(guard))
    }

    /// Explicit, Project-writer-authorized CAS publication of the current
    /// independently approved policy. No implicit grants at activation.
    pub fn publish_grant(
        &self,
        manager: &PrincipalId,
        expected_revision: u64,
    ) -> Result<SandboxGrantStatus> {
        let guard = self.read()?;
        self.store.sandbox_grant_set(
            manager,
            &guard.policy().agent_ref,
            expected_revision,
            Some(guard.policy()),
        )
    }

    /// New effects fail closed. Admitted native workers may finish while
    /// retaining their read guard; this is not a Process stop operation.
    /// This withdraws live host authority only. The existing Store CAS
    /// owner separately revokes durable grant evidence when requested.
    pub async fn revoke(&self) -> bool {
        self.current.write().await.take().is_some()
    }

    /// Exact read-side preflight for an explicit host activation.
    /// Not a reusable runtime admission token or a new permission.
    pub fn preflight(
        &self,
        store: &SessionStore,
        actor: &PrincipalId,
        agent: &AgentExecutionBinding,
    ) -> Result<SandboxGrantObservation> {
        if !self.store.shares_owner(store) {
            return Err(Error::Denied(
                "native source approval belongs to another Store owner",
            ));
        }
        let guard = self.read()?;
        let policy = guard.policy();
        if &policy.actor != actor || guard.agent() != agent {
            return Err(Error::Denied(
                "native source approval belongs to another Agent actor",
            ));
        }
        let observed = store.sandbox_grant_preflight(
            actor,
            agent.canonical_ref(),
            policy,
            guard.tool_ceiling(),
        )?;
        if observed.sources.len() != 1 || observed.sources[0].binding != *guard.source() {
            return Err(Error::Conflict("native source approval binding changed"));
        }
        Ok(observed)
    }
}
