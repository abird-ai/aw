//! Bounded transport-neutral shared prompt draft primitives.
//!
//! This crate owns only replicated plain-text mechanics and typed metadata. It
//! deliberately has no Session store, provider, tool, HTTP, filesystem, auth or
//! model-execution authority. Callers authenticate actors and authorize every
//! operation before applying updates.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use yrs::{updates::decoder::Decode, Doc, GetString, ReadTxn, StateVector, Text, Transact, Update};

pub const DOCUMENT_NAME: &str = "prompt";
pub const MAX_UPDATE_BYTES: usize = 256 * 1024;
pub const MAX_DRAFT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftMode {
    Off,
    Auto,
    On,
}

impl DraftMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::On => "on",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftState {
    Active,
    Sealed,
    Admitted,
    Rejected,
    Abandoned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftGrant {
    pub can_read: bool,
    pub can_edit: bool,
    pub can_submit: bool,
    pub can_manage: bool,
    pub presence_read: bool,
}

impl DraftGrant {
    pub const fn editor_submitter() -> Self {
        Self {
            can_read: true,
            can_edit: true,
            can_submit: true,
            can_manage: false,
            presence_read: true,
        }
    }

    pub const fn owner() -> Self {
        Self {
            can_read: true,
            can_edit: true,
            can_submit: true,
            can_manage: true,
            presence_read: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftTarget {
    pub session_id: String,
    pub branch_id: String,
    pub head: String,
    pub branch_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftSnapshot {
    pub draft_id: String,
    pub generation: u64,
    pub document_epoch: u64,
    pub accepted_revision: u64,
    pub metadata_revision: u64,
    pub state: DraftState,
    pub mode: DraftMode,
    pub target: DraftTarget,
    pub text: String,
    pub update_v1: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedCandidate {
    pub submission_id: String,
    pub draft_id: String,
    pub generation: u64,
    pub accepted_revision: u64,
    pub metadata_revision: u64,
    pub target: DraftTarget,
    pub text: String,
    pub update_v1: Vec<u8>,
    pub digest_sha256: String,
    pub submitted_by_user_id: String,
    pub contributor_user_ids: Vec<String>,
    pub status: String,
    pub prompt_request_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum CollabError {
    #[error("draft update exceeds configured bound")]
    UpdateTooLarge,
    #[error("draft text exceeds configured bound")]
    DraftTooLarge,
    #[error("invalid Yjs-compatible update")]
    InvalidUpdate,
    #[error("draft UTF-8 materialization failed")]
    Materialize,
}

pub type Result<T> = std::result::Result<T, CollabError>;

/// One bounded Yjs-compatible plain-text document. The binary state is V1 so
/// stable Yjs 13 browser clients and Yrs `small-client` mode share a tested wire.
pub struct DraftDocument {
    doc: Doc,
}

impl Default for DraftDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl DraftDocument {
    pub fn new() -> Self {
        let doc = Doc::new();
        let _ = doc.get_or_insert_text(DOCUMENT_NAME);
        Self { doc }
    }

    pub fn from_update_v1(update: &[u8]) -> Result<Self> {
        if update.len() > MAX_DRAFT_BYTES {
            return Err(CollabError::DraftTooLarge);
        }
        let doc = Self::new();
        if !update.is_empty() {
            let decoded = Update::decode_v1(update).map_err(|_| CollabError::InvalidUpdate)?;
            doc.doc
                .transact_mut()
                .apply_update(decoded)
                .map_err(|_| CollabError::InvalidUpdate)?;
        }
        let value = doc.text();
        if value.len() > MAX_DRAFT_BYTES {
            return Err(CollabError::DraftTooLarge);
        }
        Ok(doc)
    }

    pub fn apply_update_v1(&self, update: &[u8]) -> Result<()> {
        if update.is_empty() || update.len() > MAX_UPDATE_BYTES {
            return Err(CollabError::UpdateTooLarge);
        }
        let decoded = Update::decode_v1(update).map_err(|_| CollabError::InvalidUpdate)?;
        self.doc
            .transact_mut()
            .apply_update(decoded)
            .map_err(|_| CollabError::InvalidUpdate)?;
        if self.text().len() > MAX_DRAFT_BYTES {
            return Err(CollabError::DraftTooLarge);
        }
        Ok(())
    }

    pub fn text(&self) -> String {
        let text = self.doc.get_or_insert_text(DOCUMENT_NAME);
        text.get_string(&self.doc.transact())
    }

    pub fn encode_full_v1(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    pub fn replace_text(&self, value: &str) -> Result<Vec<u8>> {
        if value.len() > MAX_DRAFT_BYTES {
            return Err(CollabError::DraftTooLarge);
        }
        let text = self.doc.get_or_insert_text(DOCUMENT_NAME);
        let before = self.doc.transact().state_vector();
        {
            let mut txn = self.doc.transact_mut();
            let len = text.len(&txn);
            if len != 0 {
                text.remove_range(&mut txn, 0, len);
            }
            if !value.is_empty() {
                text.insert(&mut txn, 0, value);
            }
        }
        Ok(self.doc.transact().encode_state_as_update_v1(&before))
    }
}

pub fn candidate_digest(text: &str, target: &DraftTarget, metadata_revision: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aw-shared-draft-v1\0");
    hasher.update(text.as_bytes());
    hasher.update(b"\0");
    hasher.update(target.session_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(target.branch_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(target.head.as_bytes());
    hasher.update(b"\0");
    hasher.update(target.branch_revision.to_be_bytes());
    hasher.update(metadata_revision.to_be_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

    #[test]
    fn frozen_yjs_v1_fixture_decodes_in_yrs_without_text_change() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/shared_draft_yjs_v1.json"
        ))
        .unwrap();
        let expected = fixture["text"].as_str().unwrap();
        for key in [
            "yjs_13_6_33_update_v1_b64",
            "yrs_0_28_0_small_client_update_v1_b64",
        ] {
            let bytes = BASE64.decode(fixture[key].as_str().unwrap()).unwrap();
            let doc = DraftDocument::from_update_v1(&bytes).unwrap();
            assert_eq!(doc.text(), expected);
        }
    }

    #[test]
    fn independent_updates_converge_regardless_of_order() {
        let a = DraftDocument::new();
        let b = DraftDocument::new();
        let au = a.replace_text("α🙂").unwrap();
        let bu = b.replace_text("β").unwrap();

        let ab = DraftDocument::new();
        ab.apply_update_v1(&au).unwrap();
        ab.apply_update_v1(&bu).unwrap();
        let ba = DraftDocument::new();
        ba.apply_update_v1(&bu).unwrap();
        ba.apply_update_v1(&au).unwrap();
        assert_eq!(ab.text(), ba.text());
    }

    #[test]
    fn full_state_round_trips_unicode_without_normalization() {
        let original = "e\u{301} ≠ é 👩🏽‍💻\n\tשלום";
        let doc = DraftDocument::new();
        let _ = doc.replace_text(original).unwrap();
        let restored = DraftDocument::from_update_v1(&doc.encode_full_v1()).unwrap();
        assert_eq!(restored.text(), original);
    }

    #[test]
    fn digest_binds_text_and_exact_target() {
        let target = DraftTarget {
            session_id: "s".into(),
            branch_id: "b".into(),
            head: "n1".into(),
            branch_revision: 7,
        };
        let a = candidate_digest("prompt", &target, 1);
        let mut moved = target.clone();
        moved.branch_revision = 8;
        assert_ne!(a, candidate_digest("prompt", &moved, 1));
        assert_ne!(a, candidate_digest("prompt!", &target, 1));
        assert_ne!(a, candidate_digest("prompt", &target, 2));
    }
}
