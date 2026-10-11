//! Explicit host-configured participants. A directory lookup is NOT login.
//!
//! The host authenticates possession of an invite/session before using the
//! resolved principal. User input cannot assert an actor simply by naming it.
//! No network, password store, provider, filesystem or agent dependencies.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const MAX_PARTICIPANTS: usize = 8;
#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("invalid internal directory: {0}")]
    Invalid(&'static str),
    #[error("participant is not configured")]
    Unavailable,
}
pub type Result<T> = std::result::Result<T, IdentityError>;

/// A local realm component, not an OIDC issuer or a global Unix UID.
fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Participant {
    /// Stable immutable-in-practice account ID. Renaming it creates another identity.
    pub id: String,
    pub display_name: String,
}
impl Participant {
    pub fn validate(&self) -> Result<()> {
        if !component(&self.id) {
            return Err(IdentityError::Invalid(
                "ID must be 1..32 lowercase ASCII letters/digits/_/-",
            ));
        }
        if self.display_name.trim().is_empty()
            || self.display_name.len() > 96
            || self.display_name.chars().any(|c| {
                c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err(IdentityError::Invalid(
                "display name must be bounded readable text",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roster {
    version: u16,
    realm: String,
    participants: Vec<Participant>,
}
impl Roster {
    pub fn new(realm: impl Into<String>, participants: Vec<Participant>) -> Result<Self> {
        let value = Self {
            version: 1,
            realm: realm.into(),
            participants,
        };
        value.validate()?;
        Ok(value)
    }
    /// Always validate a deserialized roster before a trusted host uses it.
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || !component(&self.realm) {
            return Err(IdentityError::Invalid("unknown schema or invalid realm"));
        }
        if self.participants.is_empty() || self.participants.len() > MAX_PARTICIPANTS {
            return Err(IdentityError::Invalid(
                "directory requires 1..8 participants",
            ));
        }
        let mut ids = BTreeSet::new();
        for user in &self.participants {
            user.validate()?;
            if !ids.insert(&user.id) {
                return Err(IdentityError::Invalid("duplicate participant ID"));
            }
        }
        Ok(())
    }
    pub fn realm(&self) -> &str {
        &self.realm
    }
    pub fn participants(&self) -> &[Participant] {
        &self.participants
    }
    pub fn participant(&self, id: &str) -> Result<&Participant> {
        self.validate()?;
        self.participants
            .iter()
            .find(|p| p.id == id)
            .ok_or(IdentityError::Unavailable)
    }
    /// Host-side naming only. Does not authenticate the requesting party.
    pub fn principal(&self, id: &str) -> Result<String> {
        let participant = self.participant(id)?;
        Ok(format!("internal:{}:{}", self.realm, participant.id))
    }
    pub fn owns(&self, id: &str, principal: &str) -> Result<bool> {
        Ok(self.principal(id)? == principal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn user(id: &str) -> Participant {
        Participant {
            id: id.into(),
            display_name: "Name".into(),
        }
    }
    fn roster() -> Roster {
        Roster::new("realm-a", vec![user("alice"), user("bob")]).unwrap()
    }
    #[test]
    fn stable_qualified_principal() {
        assert_eq!(
            roster().principal("alice").unwrap(),
            "internal:realm-a:alice"
        );
    }
    #[test]
    fn unknown_user_denied() {
        assert!(roster().principal("mallory").is_err());
    }
    #[test]
    fn duplicate_ids_denied() {
        assert!(Roster::new("r", vec![user("a"), user("a")]).is_err());
    }
    #[test]
    fn duplicate_display_names_are_not_identity() {
        assert!(roster().validate().is_ok());
    }
    #[test]
    fn display_rename_keeps_identity() {
        let mut a = roster();
        a.participants[0].display_name = "Another name".into();
        assert_eq!(
            a.principal("alice").unwrap(),
            roster().principal("alice").unwrap()
        );
    }
    #[test]
    fn participant_rename_does_not_link_old_history() {
        assert!(!roster()
            .owns("alice", "internal:realm-a:former-name")
            .unwrap());
    }
    #[test]
    fn realm_namespaces_accounts() {
        let a = roster();
        let b = Roster::new("realm-b", vec![user("alice")]).unwrap();
        assert_ne!(a.principal("alice").unwrap(), b.principal("alice").unwrap());
    }
    #[test]
    fn unix_identity_not_inherited() {
        assert!(!roster().owns("alice", "unix:1000").unwrap());
    }
    #[test]
    fn empty_roster_denied() {
        assert!(Roster::new("r", vec![]).is_err());
    }
    #[test]
    fn directory_is_bounded() {
        assert!(Roster::new("r", (0..9).map(|i| user(&format!("u{i}"))).collect()).is_err());
    }
    #[test]
    fn path_components_denied() {
        for id in ["../alice", "a/b", "ALICE", "a:b", "a@b", " ", ""] {
            assert!(Roster::new("r", vec![user(id)]).is_err());
        }
    }
    #[test]
    fn realm_is_bounded() {
        assert!(Roster::new("r".repeat(33), vec![user("a")]).is_err());
    }
    #[test]
    fn label_is_not_html_authority() {
        let mut u = user("a");
        u.display_name = "<button>".into();
        assert!(Roster::new("r", vec![u]).is_ok());
    }
    #[test]
    fn control_labels_rejected() {
        for label in ["a\nb", "\u{1b}[2J", "\u{202e}admin", ""] {
            let mut u = user("a");
            u.display_name = label.into();
            assert!(Roster::new("r", vec![u]).is_err());
        }
    }
    #[test]
    fn deserialization_requires_validation() {
        let raw = r#"{"version":2,"realm":"r","participants":[{"id":"a","display_name":"A"}]}"#;
        let x: Roster = serde_json::from_str(raw).unwrap();
        assert!(x.validate().is_err());
    }
    #[test]
    fn unexpected_fields_rejected() {
        let raw = r#"{"version":1,"realm":"r","participants":[{"id":"a","display_name":"A","admin":true}]}"#;
        assert!(serde_json::from_str::<Roster>(raw).is_err());
    }
    #[test]
    fn round_trip() {
        let r = roster();
        let back: Roster = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        assert_eq!(
            back.principal("alice").unwrap(),
            r.principal("alice").unwrap()
        );
    }
}

#[cfg(test)]
mod vector_tests {
    use super::*;
    #[test]
    fn shared_identity_vectors() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/participants.json")).unwrap();
        for case in fixture["vectors"].as_array().unwrap() {
            let result = Roster::new(
                case["realm"].as_str().unwrap(),
                vec![Participant {
                    id: case["id"].as_str().unwrap().into(),
                    display_name: case["label"].as_str().unwrap().into(),
                }],
            );
            assert_eq!(result.is_ok(), case["valid"].as_bool().unwrap());
            if let Ok(roster) = result {
                assert_eq!(
                    roster.principal(case["id"].as_str().unwrap()).unwrap(),
                    case["principal"].as_str().unwrap()
                );
            }
        }
    }
}
