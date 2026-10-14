//! Device-owned finite consent and owner-side non-secret link description.
use crate::{
    protocol::{digest, label, model_label, Binding, Request, RECEIPT_LIMIT, RESPONSE_LIMIT},
    Error, Result,
};
use aw_agent_core::{AccountOwner, BillingOwner, CredentialCustody, InvocationScope};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|n| n.as_secs())
        .map_err(|_| Error::Denied("system clock before epoch"))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkConfig {
    pub version: u32,
    pub socket: PathBuf,
    pub key_file: PathBuf,
    pub account_id: String,
    pub executor_id: String,
    pub model: String,
    pub grant_id: String,
}
impl LinkConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let x: Self = serde_json::from_slice(&crate::private::read(path, 16384)?)
            .map_err(|_| Error::Config("invalid device-link JSON".into()))?;
        x.validate()?;
        Ok(x)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.executor_id == "local" {
            return Err(Error::Denied(
                "link must explicitly identify a nonlocal executor",
            ));
        }
        for s in [&self.account_id, &self.executor_id, &self.grant_id] {
            label(s)?;
        }
        model_label(&self.model)?;
        if !self.socket.is_absolute() || !self.key_file.is_absolute() {
            return Err(Error::Config(
                "device socket/key file must be absolute".into(),
            ));
        }
        Ok(())
    }
    pub fn binding(&self, actor: &str) -> Binding {
        Binding {
            actor: actor.into(),
            account_id: self.account_id.clone(),
            executor_id: self.executor_id.clone(),
            model: self.model.clone(),
            grant_id: self.grant_id.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub version: u32,
    pub binding: Binding,
    pub scopes: Vec<InvocationScope>,
    pub allowed_tools: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub allowed_summary_digests: BTreeSet<String>,
    pub issued_at: u64,
    pub expires_at: u64,
    pub max_calls: u32,
    pub max_context_bytes: usize,
    pub max_output_bytes: usize,
    pub max_total_output_bytes: usize,
    pub max_output_tokens: u32,
    pub timeout_seconds: u64,
}
impl Grant {
    pub fn load(path: &Path) -> Result<(Self, String)> {
        let bytes = crate::private::read(path, 65536)?;
        let g: Self = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Config("invalid finite device grant JSON".into()))?;
        g.validate(now()?)?;
        let h = digest(&g)?;
        Ok((g, h))
    }
    pub fn validate(&self, time: u64) -> Result<()> {
        self.binding.validate()?;
        if !(1..=2).contains(&self.version)
            || self.binding.executor_id == "local"
            || self.scopes.is_empty()
            || self.scopes.len() > 16
        {
            return Err(Error::Denied(
                "invalid device grant version/executor/session scopes",
            ));
        }
        for s in &self.scopes {
            label(&s.session_id)?;
            label(&s.branch_id)?;
        }
        if self.issued_at > time
            || self.expires_at <= time
            || self.expires_at <= self.issued_at
            || self.expires_at - self.issued_at > 86400
        {
            return Err(Error::Denied(
                "device grant expired, future-dated or longer than 24h",
            ));
        }
        if self.max_calls == 0
            || self.max_calls > RECEIPT_LIMIT as u32
            || !(1024..=1024 * 1024).contains(&self.max_context_bytes)
            || self.max_output_bytes == 0
            || self.max_output_bytes > RESPONSE_LIMIT
            || self.max_total_output_bytes < self.max_output_bytes
            || self.max_total_output_bytes > 16 * 1024 * 1024
            || self.max_output_tokens == 0
            || self.max_output_tokens > 32768
            || self.timeout_seconds == 0
            || self.timeout_seconds > 600
            || self.allowed_tools.len() > 32
        {
            return Err(Error::Limit("invalid device finite bounds"));
        }
        if self.allowed_summary_digests.len() > 16
            || (self.version == 1 && !self.allowed_summary_digests.is_empty())
        {
            return Err(Error::Denied(
                "summary consent needs a bounded version 2 grant",
            ));
        }
        for d in &self.allowed_summary_digests {
            if !aw_agent_core::valid_digest(d) {
                return Err(Error::Denied("summary consent digest invalid"));
            }
        }
        for t in &self.allowed_tools {
            label(t)?;
        }
        Ok(())
    }
    pub fn admit(&self, r: &Request, time: u64) -> Result<()> {
        self.validate(time)?;
        r.validate()?;
        let i = &r.context.identity;
        let s = r
            .context
            .scope
            .as_ref()
            .ok_or(Error::Denied("device scope required"))?;
        if !self.scopes.contains(s)
            || i.actor.as_str() != self.binding.actor
            || i.account_id != self.binding.account_id
            || i.model != self.binding.model
            || i.inference_executor != self.binding.executor_id
            || i.account_owner != AccountOwner::User(i.actor.clone())
            || i.payer != BillingOwner::User(i.actor.clone())
            || i.credential_custody
                != CredentialCustody::UserDevice(self.binding.executor_id.clone())
        {
            return Err(Error::Denied(
                "request outside local actor/account/session/model/device consent",
            ));
        }
        if let aw_agent_core::InvocationPurpose::Summary { input_sha256, .. } = &r.context.purpose {
            if self.version != 2 || !self.allowed_summary_digests.contains(input_sha256) {
                return Err(Error::Denied(
                    "summary-purpose evidence was not explicitly approved on this device",
                ));
            }
        }
        if r.context_bytes() > self.max_context_bytes {
            return Err(Error::Limit("local device context budget exceeded"));
        }
        if r.tools
            .iter()
            .any(|t| !self.allowed_tools.contains(&t.name))
        {
            return Err(Error::Denied("tool schema was not locally permitted"));
        }
        Ok(())
    }
    pub fn remaining_seconds(&self, time: u64) -> u64 {
        self.timeout_seconds
            .min(self.expires_at.saturating_sub(time))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    #[test]
    fn local_bounds_accept_matching_call() {
        let g = grant();
        assert!(g.admit(&request(&g), g.issued_at).is_ok());
    }
    #[test]
    fn rejects_expired_future_and_unbounded() {
        let mut g = grant();
        assert!(g.validate(g.expires_at).is_err());
        assert!(g.validate(g.issued_at - 1).is_err());
        g.expires_at = g.issued_at + 86401;
        assert!(g.validate(g.issued_at).is_err());
    }
    #[test]
    fn other_actor_denied() {
        let g = grant();
        let mut r = request(&g);
        r.context.identity.actor = aw_agent_core::PrincipalId::new("other").unwrap();
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn other_branch_denied() {
        let g = grant();
        let mut r = request(&g);
        r.context.scope.as_mut().unwrap().branch_id = "other".into();
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn other_model_denied() {
        let g = grant();
        let mut r = request(&g);
        r.context.identity.model = "other".into();
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn owner_account_cannot_impersonate_device() {
        let g = grant();
        let mut r = request(&g);
        r.context.identity.credential_custody = CredentialCustody::Server("local".into());
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn foreign_payer_denied() {
        let g = grant();
        let mut r = request(&g);
        r.context.identity.payer = BillingOwner::Shared("team".into());
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn unknown_tool_schema_denied() {
        let g = grant();
        let mut r = request(&g);
        r.tools.push(crate::protocol::WireTool {
            name: "bash".into(),
            description: "x".into(),
            parameters: serde_json::json!({}),
            effect: aw_agent_core::ToolEffect::Process,
        });
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn supplied_endpoint_field_rejected() {
        let g = grant();
        let mut v = serde_json::to_value(request(&g)).unwrap();
        v["endpoint"] = serde_json::json!("https://untrusted.invalid");
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    #[test]
    fn profile_config_is_not_a_remote_field() {
        let g = grant();
        let mut v = serde_json::to_value(request(&g)).unwrap();
        v["context"]["api_key_env"] = serde_json::json!("SECRET");
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    #[test]
    fn receipt_digest_stable_over_registry_reconnect() {
        let g = grant();
        let mut r = request(&g);
        let d = r.request_digest().unwrap();
        r.epoch = "cd".repeat(32);
        r.context.identity.grant_revision += 1;
        assert_eq!(r.request_digest().unwrap(), d);
        r.messages[0].content.push('!');
        assert_ne!(r.request_digest().unwrap(), d);
    }
    #[test]
    fn fork_cannot_reset_invocation_id() {
        let g = grant();
        let mut r = request(&g);
        let id = r.id.clone();
        r.context.scope.as_mut().unwrap().branch_id = "another".into();
        assert_eq!(Request::identity(&r.context).unwrap(), id);
    }
}

#[cfg(test)]
mod summary_consent_tests {
    use super::*;
    use crate::test_support::*;
    use aw_agent_core::{EvidenceRef, InvocationPurpose, SummaryInput};
    fn summary_request(g: &Grant) -> Request {
        let mut r = request(g);
        let refs = vec![EvidenceRef {
            authority: "a_fixture".into(),
            session_id: "s_source".into(),
            node_id: "n_source".into(),
            sha256: "a".repeat(64),
        }];
        let input=SummaryInput{plan_id:"cp_fixture".into(),focus:"Review findings".into(),evidence_json:serde_json::json!({"dependencies":refs,"entries":[{"text":"visible fixture evidence"}]}).to_string(),sources:refs.clone(),max_output_bytes:1024,timeout_seconds:10};
        let messages = input.messages().unwrap();
        r.messages = messages
            .iter()
            .map(crate::protocol::WireMessage::from_message)
            .collect();
        r.context.purpose = InvocationPurpose::Summary {
            operation_id: "cs_fixture".into(),
            input_sha256: input.digest().unwrap(),
            sources: refs,
        };
        r
    }
    #[test]
    fn old_grant_stays_chat_only() {
        let g = grant();
        let r = summary_request(&g);
        assert!(r.validate().is_ok());
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn exact_local_evidence_digest_is_required() {
        let mut g = grant();
        g.version = 2;
        let r = summary_request(&g);
        assert!(g.admit(&r, g.issued_at).is_err());
        if let InvocationPurpose::Summary { input_sha256, .. } = &r.context.purpose {
            g.allowed_summary_digests.insert(input_sha256.clone());
        }
        assert!(g.admit(&r, g.issued_at).is_ok());
    }
    #[test]
    fn consent_never_survives_evidence_or_focus_changes() {
        let mut g = grant();
        g.version = 2;
        let mut r = summary_request(&g);
        if let InvocationPurpose::Summary { input_sha256, .. } = &r.context.purpose {
            g.allowed_summary_digests.insert(input_sha256.clone());
        }
        r.messages[1].content.push(' ');
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn manifest_substitution_does_not_reuse_consent() {
        let mut g = grant();
        g.version = 2;
        let mut r = summary_request(&g);
        if let InvocationPurpose::Summary {
            input_sha256,
            sources,
            ..
        } = &mut r.context.purpose
        {
            g.allowed_summary_digests.insert(input_sha256.clone());
            sources[0].node_id = "n_different".into();
        }
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn summary_with_tool_schema_is_invalid_even_when_tool_is_allowed() {
        let mut g = grant();
        g.version = 2;
        g.allowed_tools.insert("read".into());
        let mut r = summary_request(&g);
        if let InvocationPurpose::Summary { input_sha256, .. } = &r.context.purpose {
            g.allowed_summary_digests.insert(input_sha256.clone());
        }
        r.tools.push(crate::protocol::WireTool {
            name: "read".into(),
            description: "fixture".into(),
            parameters: serde_json::json!({}),
            effect: aw_agent_core::ToolEffect::Read,
        });
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn adding_summary_scope_does_not_expand_destination_branch() {
        let mut g = grant();
        g.version = 2;
        let mut r = summary_request(&g);
        if let InvocationPurpose::Summary { input_sha256, .. } = &r.context.purpose {
            g.allowed_summary_digests.insert(input_sha256.clone());
        }
        r.context.scope.as_mut().unwrap().branch_id = "b_other".into();
        assert!(g.admit(&r, g.issued_at).is_err());
    }
    #[test]
    fn version_one_empty_field_preserves_serialization_shape() {
        let g = grant();
        assert!(!serde_json::to_string(&g)
            .unwrap()
            .contains("allowed_summary_digests"));
    }
}
