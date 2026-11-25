//! Independently approved named native source directories. One actual
//! live policy owner, not a registry of copied grants.
use crate::source_filesystem::{require_named_root, NativeSourceRoot};
use aw_agent_core::{
    AgentExecutionBinding, Error, NamedSourceAccess, PrincipalId, Result, SourceAccess,
};
use aw_store::{
    AgentLifecycle, EffectiveSourceBinding, SandboxEffectAuthority, SandboxGrantObservation,
    SandboxGrantPolicy, SandboxGrantStatus, SandboxPolicyEvidence, SandboxSourceGrant,
    SessionStore, VersionedSourceBinding,
};
use aw_tools_adapter::native_filesystem::{
    CapabilitySet, GrantSpec, PreparedGrant, PrivateArtifactRegistry,
};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{OwnedRwLockReadGuard, RwLock};

/// Explicit trusted-host approval. None of these inputs are model arguments
/// or reconstructed from a persisted SandboxGrant. The host owns the stable
/// authority identity/generation and must not reuse a revoked generation.
pub struct NativeSourceApprovalSpec<D = NativeSourceDirectorySpec> {
    pub agent_ref: String,
    pub actor: PrincipalId,
    pub policy_ref: String,
    pub authority_ref: String,
    pub authority_generation: u64,
    pub base: PathBuf,
    pub directories: Vec<D>,
}

pub struct NativeSourceDirectorySpec {
    pub source: VersionedSourceBinding,
    pub directory: GrantSpec,
    pub private_artifacts: Arc<PrivateArtifactRegistry>,
}

impl NativeSourceDirectorySpec {
    /// Trusted provisioning of the actual persistent artifact owner.
    /// This neither grants source access nor performs physical cleanup.
    pub fn durable(
        source: VersionedSourceBinding,
        directory: GrantSpec,
        state_root: &Path,
    ) -> Result<Self> {
        let private_artifacts = aw_tools_adapter::native_filesystem::open_source_artifacts(
            &directory.path,
            state_root,
        )?;
        Ok(Self {
            source,
            directory,
            private_artifacts,
        })
    }
}

struct Decision {
    agent: AgentExecutionBinding,
    policy: SandboxGrantPolicy,
    roots: Vec<NativeSourceRoot>,
    tool_ceiling: Vec<NamedSourceAccess>,
    // Retain the exact approved directory objects, independent of names.
    // Never use this maximum capability directly for a tool effect.
    pin: CapabilitySet,
}

/// Live host policy for one exact Agent and named source set. Revocation is
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

    pub fn base(&self) -> &Path {
        self.decision().pin.base()
    }

    pub fn roots(&self) -> &[NativeSourceRoot] {
        &self.decision().roots
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
    /// Pin independently approved exact roots. No source materialization
    /// or private-artifact reconciliation; one original artifact owner
    /// per directory. The trusted host must exclude physical aliases.
    pub fn pin(store: SessionStore, mut spec: NativeSourceApprovalSpec) -> Result<Self> {
        store.source_set_limit().admit(spec.directories.len())?;
        if spec.directories.is_empty()
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
        spec.directories.sort_unstable_by(|a, b| {
            a.source
                .binding
                .name
                .as_str()
                .cmp(b.source.binding.name.as_str())
        });
        if spec
            .directories
            .windows(2)
            .any(|pair| pair[0].source.binding.name == pair[1].source.binding.name)
        {
            return Err(Error::Conflict("duplicate named native source approval"));
        }
        let mode = |write| {
            if write {
                SourceAccess::ReadWrite
            } else {
                SourceAccess::ReadOnly
            }
        };
        for directory in &spec.directories {
            directory.source.binding.validate()?;
            require_named_root(
                &spec.base,
                &directory.source.binding.name,
                &directory.directory.path,
                spec.directories.len(),
            )?;
            if directory.source.binding_rev == 0
                || !directory.directory.read
                || !directory
                    .source
                    .binding
                    .access
                    .permits(mode(directory.directory.write))
            {
                return Err(Error::Denied("invalid native source directory approval"));
            }
        }
        let pin = CapabilitySet::open_prepared_single_link_with_exclusions(
            &spec.base,
            spec.directories.iter().map(|directory| {
                PreparedGrant::new(
                    directory.directory.clone(),
                    directory.private_artifacts.clone(),
                )
            }),
            std::iter::empty(),
        )
        .map_err(|_| {
            Error::Denied("independently approved native directories could not be pinned")
        })?;
        let identities = pin
            .identities()
            .map_err(|_| Error::Denied("approved native directory identity unavailable"))?;
        if pin.base() != spec.base || identities.len() != spec.directories.len() {
            return Err(Error::Denied(
                "native source approval cannot follow directory aliases",
            ));
        }
        let mut roots = Vec::with_capacity(identities.len());
        for (directory, physical) in spec.directories.into_iter().zip(identities) {
            if physical.path != directory.directory.path
                || !physical.read
                || physical.write != directory.directory.write
            {
                return Err(Error::Denied(
                    "native source approval cannot follow directory aliases",
                ));
            }
            roots.push(NativeSourceRoot {
                source: EffectiveSourceBinding {
                    binding: directory.source,
                    access: mode(physical.write),
                },
                physical,
                private_artifacts: directory.private_artifacts,
            });
        }
        // One canonical policy digest over the actual approved directory
        // objects and binding revisions. Preserve the original one-root
        // encoding when the root itself is the layout.
        let encode_root = |root: &NativeSourceRoot| {
            serde_json::to_vec(&(
                &root.source.binding,
                root.source.access,
                &root.physical.path,
                root.physical.volume,
                root.physical.object,
            ))
            .map_err(|_| Error::Storage("native source approval identity cannot be encoded"))
        };
        let mut digest = Sha256::new();
        if roots.len() == 1 && roots[0].physical.path == spec.base {
            digest.update(b"aw-native-source-approval-v1\0");
            digest.update(encode_root(&roots[0])?);
        } else {
            digest.update(b"aw-native-source-approval-layout-v1\0");
            let base = serde_json::to_vec(&spec.base)
                .map_err(|_| Error::Storage("native layout identity cannot be encoded"))?;
            digest.update((base.len() as u64).to_be_bytes());
            digest.update(base);
            for root in &roots {
                let encoded = encode_root(root)?;
                digest.update((encoded.len() as u64).to_be_bytes());
                digest.update(encoded);
            }
        }
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
            sources: roots
                .iter()
                .map(|root| {
                    SandboxSourceGrant::from_binding(&root.source.binding, root.source.access)
                })
                .collect(),
        };
        let tool_ceiling = roots
            .iter()
            .map(|root| NamedSourceAccess {
                name: root.source.binding.binding.name.clone(),
                access: root.source.access,
            })
            .collect();
        Ok(Self {
            store,
            current: Arc::new(RwLock::new(Some(Decision {
                agent: execution,
                policy,
                roots,
                tool_ceiling,
                pin,
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
        if observed.sources.len() != guard.roots().len()
            || observed
                .sources
                .iter()
                .zip(guard.roots())
                .any(|(source, root)| source.binding != root.source.binding)
        {
            return Err(Error::Conflict("native source approval bindings changed"));
        }
        Ok(observed)
    }
}
