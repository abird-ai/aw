//! Host-neutral authenticated remote-Start evidence.
//!
//! The value is not a bearer grant by itself. AW commits it under current
//! authority; a concrete remote owner accepts it only over its authenticated
//! bridge, claims the exact authorization id through its existing replay owner,
//! and independently rechecks local native authority.

use crate::{execution_ref::ExecutionRef, operation::OperationId};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const REMOTE_START_VERSION: u16 = 1;
pub const MAX_REMOTE_START_LEASE_MILLIS: u64 = 120_000;

const MAX_HOST_BYTES: usize = 256;
const MAX_PROFILE_BYTES: usize = 128;
const MAX_INVOCATION_BYTES: usize = 512;
const MAX_ATTEMPT_BYTES: usize = 256;
const MAX_CALL_BYTES: usize = 256;
const MAX_SOURCE_BYTES: usize = 256;
const MAX_SITE_BYTES: usize = 256;
const MAX_SCOPE_BYTES: usize = 2_048;
const MAX_SESSION_BYTES: usize = 256;
const MAX_BRANCH_BYTES: usize = 256;
const MAX_ADMISSION_BYTES: usize = 128;

fn bounded_ascii(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.is_ascii()
        && !value.chars().any(char::is_control)
}

fn put_bytes(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}

fn put_str(hash: &mut Sha256, value: &str) {
    put_bytes(hash, value.as_bytes());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSessionAction {
    Submit,
    Control,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteRuntimeLease {
    pub target_host: String,
    pub target_profile: String,
    pub generation: [u8; 32],
    /// Target-dotlink wall clock. The target issues and later evaluates both
    /// timestamps; AW never assumes synchronized clocks.
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

impl RemoteRuntimeLease {
    pub fn validate(&self) -> Result<(), RemoteStartError> {
        if !bounded_ascii(&self.target_host, MAX_HOST_BYTES)
            || !bounded_ascii(&self.target_profile, MAX_PROFILE_BYTES)
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self
                .expires_at_unix_ms
                .saturating_sub(self.issued_at_unix_ms)
                > MAX_REMOTE_START_LEASE_MILLIS
        {
            return Err(RemoteStartError::InvalidLease);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteStartIntent {
    pub version: u16,
    pub original_invocation_id: String,
    pub turn_id: u64,
    pub call_id: String,
    pub effect_attempt_id: String,
    pub operation: String,
    pub canonical_ref: String,
    pub arguments_sha256: [u8; 32],
    /// Digest of the original semantic request replay claim, not merely args.
    pub original_request_sha256: [u8; 32],
    pub session_action: RemoteSessionAction,
    pub target: RemoteRuntimeLease,
}

impl RemoteStartIntent {
    pub fn validate(&self) -> Result<(), RemoteStartError> {
        if self.version != REMOTE_START_VERSION
            || !bounded_ascii(&self.original_invocation_id, MAX_INVOCATION_BYTES)
            || self.turn_id == 0
            || !bounded_ascii(&self.call_id, MAX_CALL_BYTES)
            || !bounded_ascii(&self.effect_attempt_id, MAX_ATTEMPT_BYTES)
            || !bounded_ascii(&self.operation, 64)
        {
            return Err(RemoteStartError::InvalidIntent);
        }
        OperationId::from_semantic_name(&self.operation)
            .ok_or(RemoteStartError::InvalidOperation)?;
        let reference =
            ExecutionRef::parse(&self.canonical_ref).map_err(|_| RemoteStartError::InvalidRef)?;
        if !reference.is_canonical() {
            return Err(RemoteStartError::InvalidRef);
        }
        self.target.validate()
    }

    pub fn attempt_key(&self, actor: &str, session_id: &str) -> Result<[u8; 32], RemoteStartError> {
        self.validate()?;
        if !bounded_ascii(actor, 256) || !bounded_ascii(session_id, MAX_SESSION_BYTES) {
            return Err(RemoteStartError::InvalidProvenance);
        }
        let mut hash = Sha256::new();
        hash.update(b"aw-remote-start-attempt-v1\0");
        put_str(&mut hash, actor);
        put_str(&mut hash, session_id);
        put_str(&mut hash, &self.original_invocation_id);
        put_str(&mut hash, &self.effect_attempt_id);
        Ok(hash.finalize().into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteStartSource {
    pub source_kind: String,
    pub source_id: String,
    pub source_epoch: u64,
    pub site: String,
    pub resource_scope: String,
    pub grant_sha256: [u8; 32],
}

impl RemoteStartSource {
    pub fn validate(&self) -> Result<(), RemoteStartError> {
        if !matches!(self.source_kind.as_str(), "host" | "mcp" | "extension")
            || !bounded_ascii(&self.source_id, MAX_SOURCE_BYTES)
            || self.source_epoch == 0
            || !bounded_ascii(&self.site, MAX_SITE_BYTES)
            || !bounded_ascii(&self.resource_scope, MAX_SCOPE_BYTES)
        {
            return Err(RemoteStartError::InvalidProvenance);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteStartProvenance {
    pub actor: String,
    pub source: RemoteStartSource,
    pub policy_revision_sha256: [u8; 32],
    pub policy_admission_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub writer_epoch: u64,
    pub membership_epoch: u64,
    pub audience_epoch: u64,
}

impl RemoteStartProvenance {
    pub fn validate(&self) -> Result<(), RemoteStartError> {
        self.source.validate()?;
        if !bounded_ascii(&self.actor, 256)
            || !bounded_ascii(&self.policy_admission_id, MAX_ADMISSION_BYTES)
            || !bounded_ascii(&self.session_id, MAX_SESSION_BYTES)
            || !bounded_ascii(&self.branch_id, MAX_BRANCH_BYTES)
            || self.writer_epoch == 0
            || self.membership_epoch == 0
            || self.audience_epoch == 0
        {
            return Err(RemoteStartError::InvalidProvenance);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteStartAuthorization {
    pub authorization_id: [u8; 32],
    pub intent: RemoteStartIntent,
    pub provenance: RemoteStartProvenance,
}

impl RemoteStartAuthorization {
    pub fn new(
        intent: RemoteStartIntent,
        provenance: RemoteStartProvenance,
    ) -> Result<Self, RemoteStartError> {
        intent.validate()?;
        provenance.validate()?;
        let authorization_id = Self::digest_parts(&intent, &provenance);
        Ok(Self {
            authorization_id,
            intent,
            provenance,
        })
    }

    pub fn validate(&self) -> Result<(), RemoteStartError> {
        self.intent.validate()?;
        self.provenance.validate()?;
        if self.authorization_id != Self::digest_parts(&self.intent, &self.provenance) {
            return Err(RemoteStartError::DigestMismatch);
        }
        Ok(())
    }

    pub fn validate_for_target(
        &self,
        host: &str,
        profile: &str,
        generation: &[u8; 32],
        target_now_unix_ms: u64,
    ) -> Result<(), RemoteStartError> {
        self.validate()?;
        if self.intent.target.target_host != host
            || self.intent.target.target_profile != profile
            || &self.intent.target.generation != generation
        {
            return Err(RemoteStartError::WrongTarget);
        }
        if target_now_unix_ms < self.intent.target.issued_at_unix_ms
            || target_now_unix_ms >= self.intent.target.expires_at_unix_ms
        {
            return Err(RemoteStartError::Expired);
        }
        Ok(())
    }

    pub fn matches_intent(&self, intent: &RemoteStartIntent) -> bool {
        &self.intent == intent
    }

    fn digest_parts(intent: &RemoteStartIntent, provenance: &RemoteStartProvenance) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-remote-start-authorization-v1\0");
        hash.update(intent.version.to_be_bytes());
        put_str(&mut hash, &intent.original_invocation_id);
        hash.update(intent.turn_id.to_be_bytes());
        put_str(&mut hash, &intent.call_id);
        put_str(&mut hash, &intent.effect_attempt_id);
        put_str(&mut hash, &intent.operation);
        put_str(&mut hash, &intent.canonical_ref);
        hash.update(intent.arguments_sha256);
        hash.update(intent.original_request_sha256);
        hash.update([match intent.session_action {
            RemoteSessionAction::Submit => 1,
            RemoteSessionAction::Control => 2,
        }]);
        put_str(&mut hash, &intent.target.target_host);
        put_str(&mut hash, &intent.target.target_profile);
        hash.update(intent.target.generation);
        hash.update(intent.target.issued_at_unix_ms.to_be_bytes());
        hash.update(intent.target.expires_at_unix_ms.to_be_bytes());

        put_str(&mut hash, &provenance.actor);
        put_str(&mut hash, &provenance.source.source_kind);
        put_str(&mut hash, &provenance.source.source_id);
        hash.update(provenance.source.source_epoch.to_be_bytes());
        put_str(&mut hash, &provenance.source.site);
        put_str(&mut hash, &provenance.source.resource_scope);
        hash.update(provenance.source.grant_sha256);
        hash.update(provenance.policy_revision_sha256);
        put_str(&mut hash, &provenance.policy_admission_id);
        put_str(&mut hash, &provenance.session_id);
        put_str(&mut hash, &provenance.branch_id);
        hash.update(provenance.writer_epoch.to_be_bytes());
        hash.update(provenance.membership_epoch.to_be_bytes());
        hash.update(provenance.audience_epoch.to_be_bytes());
        hash.finalize().into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteStartError {
    InvalidLease,
    InvalidIntent,
    InvalidOperation,
    InvalidRef,
    InvalidProvenance,
    DigestMismatch,
    WrongTarget,
    Expired,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent() -> RemoteStartIntent {
        RemoteStartIntent {
            version: REMOTE_START_VERSION,
            original_invocation_id: "turn:7/call:c9".into(),
            turn_id: 7,
            call_id: "c9".into(),
            effect_attempt_id: "effect-1".into(),
            operation: "bash".into(),
            canonical_ref: "/d:abird/i:pvl/p:repo/a:0/t:7".into(),
            arguments_sha256: [1; 32],
            original_request_sha256: [2; 32],
            session_action: RemoteSessionAction::Submit,
            target: RemoteRuntimeLease {
                target_host: "l5".into(),
                target_profile: "default".into(),
                generation: [3; 32],
                issued_at_unix_ms: 10_000,
                expires_at_unix_ms: 20_000,
            },
        }
    }

    fn provenance() -> RemoteStartProvenance {
        RemoteStartProvenance {
            actor: "local:pvl".into(),
            source: RemoteStartSource {
                source_kind: "host".into(),
                source_id: "trusted-host".into(),
                source_epoch: 4,
                site: "dotlink:l5".into(),
                resource_scope: "/d:abird/i:pvl/p:repo".into(),
                grant_sha256: [4; 32],
            },
            policy_revision_sha256: [5; 32],
            policy_admission_id: "pa_fixture".into(),
            session_id: "session".into(),
            branch_id: "main".into(),
            writer_epoch: 6,
            membership_epoch: 7,
            audience_epoch: 8,
        }
    }

    #[test]
    fn exact_authorization_is_self_consistent_and_target_generation_fenced() {
        let authorization =
            RemoteStartAuthorization::new(intent(), provenance()).expect("valid authorization");
        authorization.validate().unwrap();
        authorization
            .validate_for_target("l5", "default", &[3; 32], 15_000)
            .unwrap();
        assert_eq!(
            authorization.validate_for_target("l5", "default", &[9; 32], 15_000),
            Err(RemoteStartError::WrongTarget)
        );
        assert_eq!(
            authorization.validate_for_target("l5", "default", &[3; 32], 20_000),
            Err(RemoteStartError::Expired)
        );
    }

    #[test]
    fn authorization_digest_binds_attempt_args_target_and_provenance() {
        let base = RemoteStartAuthorization::new(intent(), provenance()).unwrap();

        let mut changed = intent();
        changed.arguments_sha256[0] ^= 1;
        assert_ne!(
            base.authorization_id,
            RemoteStartAuthorization::new(changed, provenance())
                .unwrap()
                .authorization_id
        );

        let mut changed = intent();
        changed.target.generation[0] ^= 1;
        assert_ne!(
            base.authorization_id,
            RemoteStartAuthorization::new(changed, provenance())
                .unwrap()
                .authorization_id
        );

        let mut changed_provenance = provenance();
        changed_provenance.writer_epoch += 1;
        assert_ne!(
            base.authorization_id,
            RemoteStartAuthorization::new(intent(), changed_provenance)
                .unwrap()
                .authorization_id
        );
    }

    #[test]
    fn invalid_or_contextual_intent_and_unbounded_target_lease_fail_closed() {
        let mut contextual = intent();
        contextual.canonical_ref = "t:7".into();
        assert_eq!(contextual.validate(), Err(RemoteStartError::InvalidRef));

        let mut legacy_operation = intent();
        legacy_operation.operation = "process.bash".into();
        assert_eq!(
            legacy_operation.validate(),
            Err(RemoteStartError::InvalidOperation)
        );

        let mut long_lease = intent();
        long_lease.target.expires_at_unix_ms =
            long_lease.target.issued_at_unix_ms + MAX_REMOTE_START_LEASE_MILLIS + 1;
        assert_eq!(long_lease.validate(), Err(RemoteStartError::InvalidLease));
    }

    #[test]
    fn attempt_key_is_context_independent_of_current_authority_generations() {
        let intent = intent();
        let first = intent.attempt_key("local:pvl", "session").unwrap();
        let mut provenance = provenance();
        provenance.writer_epoch += 1;
        let authorization = RemoteStartAuthorization::new(intent.clone(), provenance).unwrap();
        assert_eq!(
            first,
            authorization
                .intent
                .attempt_key("local:pvl", "session")
                .unwrap()
        );
    }
}
