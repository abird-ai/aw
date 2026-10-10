//! Transport-neutral bounded main-target routing strategy types.
//!
//! This crate owns no provider, credential, store, filesystem, network, tool,
//! Session or authentication authority. A host must resolve every target
//! reference against its current authorized provider/account registry.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use thiserror::Error;

pub const MAX_ROUTING_CANDIDATES: usize = 16;
pub const MAX_ROUTING_INPUT_BYTES: usize = 128 * 1024;
pub const MAX_ROUTING_OUTPUT_BYTES: usize = 4 * 1024;
pub const MAX_ROUTING_DESCRIPTION_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Fixed,
    RouteOnce,
    RouteEachPrompt,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingTargetRef {
    /// Strategy-local opaque display/selection token. Never an endpoint.
    pub id: String,
    /// Existing provider-account/profile ID resolved by the trusted host.
    pub account_id: String,
    /// Exact configured model ID resolved by the trusted host.
    pub model: String,
    /// Secret-free pin of endpoint/custody reference configuration. Hosts derive
    /// this from the configured profile before persisting an immutable revision.
    #[serde(default)]
    pub destination_fingerprint: String,
    /// Bounded routing-only description. Never instructions or authority.
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingStrategyDefinition {
    pub name: String,
    #[serde(default = "default_role")]
    pub role: String,
    pub mode: RoutingMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier: Option<RoutingTargetRef>,
    pub candidates: Vec<RoutingTargetRef>,
}

fn default_role() -> String {
    "main".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingCandidateOption {
    pub id: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingEvidence {
    pub strategy_revision_id: String,
    pub role: String,
    pub lineage: String,
    pub prompt: String,
    pub candidates: Vec<RoutingCandidateOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingProposal {
    pub candidate_id: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RoutingError {
    #[error("routing strategy is invalid: {0}")]
    Strategy(&'static str),
    #[error("routing evidence exceeds configured bound")]
    EvidenceLimit,
    #[error("routing proposal is invalid")]
    InvalidProposal,
    #[error("routing proposal selected an unavailable candidate")]
    UnknownCandidate,
}

pub type Result<T> = std::result::Result<T, RoutingError>;

fn portable_label(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-@".contains(&b))
}

fn validate_target(target: &RoutingTargetRef) -> Result<()> {
    if !portable_label(&target.id, 128)
        || !portable_label(&target.account_id, 128)
        || target.model.is_empty()
        || target.model.len() > 256
        || target.model.chars().any(char::is_control)
        || (!target.destination_fingerprint.is_empty()
            && (target.destination_fingerprint.len() != 64
                || !target
                    .destination_fingerprint
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())))
        || target.description.len() > MAX_ROUTING_DESCRIPTION_BYTES
        || target.description.chars().any(|c| c == '\0')
    {
        return Err(RoutingError::Strategy("invalid target reference"));
    }
    Ok(())
}

pub fn validate_strategy(strategy: &RoutingStrategyDefinition) -> Result<()> {
    if !portable_label(&strategy.name, 128) || !portable_label(&strategy.role, 64) {
        return Err(RoutingError::Strategy("invalid strategy name/role"));
    }
    if strategy.candidates.is_empty() || strategy.candidates.len() > MAX_ROUTING_CANDIDATES {
        return Err(RoutingError::Strategy("candidate count must be 1..16"));
    }
    let mut ids = BTreeSet::new();
    for candidate in &strategy.candidates {
        validate_target(candidate)?;
        if !ids.insert(candidate.id.as_str()) {
            return Err(RoutingError::Strategy("candidate IDs must be unique"));
        }
    }
    match strategy.mode {
        RoutingMode::Fixed => {
            if strategy.classifier.is_some() {
                return Err(RoutingError::Strategy(
                    "fixed routing cannot configure a classifier",
                ));
            }
            let fixed = strategy
                .fixed_target
                .as_deref()
                .ok_or(RoutingError::Strategy("fixed routing needs fixed_target"))?;
            if !ids.contains(fixed) {
                return Err(RoutingError::Strategy("fixed_target must name a candidate"));
            }
        }
        RoutingMode::RouteOnce | RoutingMode::RouteEachPrompt => {
            if strategy.fixed_target.is_some() || strategy.candidates.len() < 2 {
                return Err(RoutingError::Strategy(
                    "classified routing needs at least two candidates and no fixed_target",
                ));
            }
            let classifier = strategy.classifier.as_ref().ok_or(RoutingError::Strategy(
                "classified routing needs an explicit classifier target",
            ))?;
            validate_target(classifier)?;
            if ids.contains(classifier.id.as_str()) {
                return Err(RoutingError::Strategy(
                    "classifier ID must be distinct from candidate IDs",
                ));
            }
        }
    }
    Ok(())
}

pub fn strategy_sha256(strategy: &RoutingStrategyDefinition) -> Result<String> {
    validate_strategy(strategy)?;
    let encoded = serde_json::to_vec(strategy)
        .map_err(|_| RoutingError::Strategy("strategy encoding failed"))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub fn candidate_options(
    strategy: &RoutingStrategyDefinition,
) -> Result<Vec<RoutingCandidateOption>> {
    validate_strategy(strategy)?;
    Ok(strategy
        .candidates
        .iter()
        .map(|candidate| RoutingCandidateOption {
            id: candidate.id.clone(),
            description: candidate.description.clone(),
        })
        .collect())
}

pub fn target<'a>(
    strategy: &'a RoutingStrategyDefinition,
    id: &str,
) -> Result<&'a RoutingTargetRef> {
    validate_strategy(strategy)?;
    strategy
        .candidates
        .iter()
        .find(|candidate| candidate.id == id)
        .ok_or(RoutingError::UnknownCandidate)
}

impl RoutingEvidence {
    pub fn validate(&self) -> Result<()> {
        if !portable_label(&self.strategy_revision_id, 128)
            || !portable_label(&self.role, 64)
            || !portable_label(&self.lineage, 128)
            || self.prompt.trim().is_empty()
            || self.prompt.len() > MAX_ROUTING_INPUT_BYTES
            || self.candidates.is_empty()
            || self.candidates.len() > MAX_ROUTING_CANDIDATES
        {
            return Err(RoutingError::EvidenceLimit);
        }
        let mut ids = BTreeSet::new();
        for candidate in &self.candidates {
            if !portable_label(&candidate.id, 128)
                || candidate.description.len() > MAX_ROUTING_DESCRIPTION_BYTES
                || !ids.insert(candidate.id.as_str())
            {
                return Err(RoutingError::EvidenceLimit);
            }
        }
        if self.user_json()?.len() > MAX_ROUTING_INPUT_BYTES {
            return Err(RoutingError::EvidenceLimit);
        }
        Ok(())
    }

    /// Classifier-visible evidence contains candidate IDs/descriptions only.
    /// Provider account/model/endpoint/billing metadata are deliberately absent.
    pub fn user_json(&self) -> Result<String> {
        let encoded = serde_json::to_string(self).map_err(|_| RoutingError::EvidenceLimit)?;
        if encoded.len() > MAX_ROUTING_INPUT_BYTES {
            return Err(RoutingError::EvidenceLimit);
        }
        Ok(encoded)
    }

    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "{:x}",
            Sha256::digest(self.user_json()?.as_bytes())
        ))
    }

    pub fn candidate_ids_sha256(&self) -> Result<String> {
        self.validate()?;
        let ids = self
            .candidates
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<Vec<_>>();
        let encoded = serde_json::to_vec(&ids).map_err(|_| RoutingError::EvidenceLimit)?;
        Ok(format!("{:x}", Sha256::digest(encoded)))
    }
}

pub fn parse_proposal(bytes: &str, allowed: &[RoutingCandidateOption]) -> Result<RoutingProposal> {
    if bytes.is_empty() || bytes.len() > MAX_ROUTING_OUTPUT_BYTES {
        return Err(RoutingError::InvalidProposal);
    }
    let proposal: RoutingProposal =
        serde_json::from_str(bytes.trim()).map_err(|_| RoutingError::InvalidProposal)?;
    if !portable_label(&proposal.candidate_id, 128) {
        return Err(RoutingError::InvalidProposal);
    }
    if !allowed
        .iter()
        .any(|candidate| candidate.id == proposal.candidate_id)
    {
        return Err(RoutingError::UnknownCandidate);
    }
    Ok(proposal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, account: &str, model: &str) -> RoutingTargetRef {
        RoutingTargetRef {
            id: id.into(),
            account_id: account.into(),
            model: model.into(),
            destination_fingerprint: "a".repeat(64),
            description: format!("{id} workload"),
        }
    }

    fn routed(mode: RoutingMode) -> RoutingStrategyDefinition {
        RoutingStrategyDefinition {
            name: "coding".into(),
            role: "main".into(),
            mode,
            fixed_target: None,
            classifier: Some(candidate("router", "router-account", "router-model")),
            candidates: vec![
                candidate("fast", "fast-account", "fast-model"),
                candidate("deep", "deep-account", "deep-model"),
            ],
        }
    }

    #[test]
    fn fixed_strategy_needs_known_candidate_and_no_classifier() {
        let mut strategy = routed(RoutingMode::Fixed);
        strategy.classifier = None;
        strategy.fixed_target = Some("fast".into());
        validate_strategy(&strategy).unwrap();
        strategy.fixed_target = Some("missing".into());
        assert!(validate_strategy(&strategy).is_err());
    }

    #[test]
    fn classify_modes_require_two_candidates_and_explicit_classifier() {
        let mut strategy = routed(RoutingMode::RouteOnce);
        validate_strategy(&strategy).unwrap();
        strategy.classifier = None;
        assert!(validate_strategy(&strategy).is_err());
    }

    #[test]
    fn digest_changes_with_routing_semantics() {
        let a = routed(RoutingMode::RouteOnce);
        let mut b = a.clone();
        b.mode = RoutingMode::RouteEachPrompt;
        assert_ne!(strategy_sha256(&a).unwrap(), strategy_sha256(&b).unwrap());
    }

    #[test]
    fn classifier_evidence_omits_account_and_model_metadata() {
        let strategy = routed(RoutingMode::RouteOnce);
        let evidence = RoutingEvidence {
            strategy_revision_id: "rrev_1".into(),
            role: "main".into(),
            lineage: "main".into(),
            prompt: "fix the parser".into(),
            candidates: candidate_options(&strategy).unwrap(),
        };
        let json = evidence.user_json().unwrap();
        assert!(json.contains("fast workload"));
        assert!(!json.contains("fast-account"));
        assert!(!json.contains("fast-model"));
    }

    #[test]
    fn proposal_is_strict_json_and_must_select_allowed_candidate() {
        let allowed = candidate_options(&routed(RoutingMode::RouteOnce)).unwrap();
        assert_eq!(
            parse_proposal(r#"{"candidate_id":"deep"}"#, &allowed)
                .unwrap()
                .candidate_id,
            "deep"
        );
        assert!(parse_proposal(r#"{"candidate_id":"missing"}"#, &allowed).is_err());
        assert!(parse_proposal(r#"{"candidate_id":"deep","extra":true}"#, &allowed).is_err());
        assert!(parse_proposal("deep", &allowed).is_err());
    }
}
