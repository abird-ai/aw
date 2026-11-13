//! Explicit shared-Session participation authority.
//!
//! These predicates govern Session collaboration only. They never grant a
//! filesystem/process/model/provider/payer capability. Legacy members.can_append
//! remains a separate compatibility authority for legacy append APIs.

use crate::{
    audit, db, durability, ensure_user_for_principal, now, principal_is_session_owner, session_in,
    session_owner_user, user_for_principal, SessionStore,
};
use aw_agent_core::{Error, PrincipalId, Result};
use aw_store_api::{BranchInfo, SessionBinding, SessionInfo};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionParticipationAction {
    View,
    Contribute,
    Submit,
    Control,
    Manage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionParticipationPermissions {
    pub view: bool,
    pub contribute: bool,
    pub submit: bool,
    pub control: bool,
    pub manage: bool,
}

impl SessionParticipationPermissions {
    pub const ALL: Self = Self {
        view: true,
        contribute: true,
        submit: true,
        control: true,
        manage: true,
    };

    pub const VIEW_ONLY: Self = Self {
        view: true,
        contribute: false,
        submit: false,
        control: false,
        manage: false,
    };

    pub const fn allows(self, action: SessionParticipationAction) -> bool {
        match action {
            SessionParticipationAction::View => self.view,
            SessionParticipationAction::Contribute => self.contribute,
            SessionParticipationAction::Submit => self.submit,
            SessionParticipationAction::Control => self.control,
            SessionParticipationAction::Manage => self.manage,
        }
    }

    pub const fn is_empty(self) -> bool {
        !self.view && !self.contribute && !self.submit && !self.control && !self.manage
    }

    fn validate(self) -> Result<Self> {
        if !self.view && (self.contribute || self.submit || self.control || self.manage) {
            return Err(Error::Denied(
                "Session participation actions require explicit View authority",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionParticipationEvidence {
    pub session_id: String,
    pub actor: PrincipalId,
    pub permissions: SessionParticipationPermissions,
    /// Per-participant grant/security generation. A changed/revoked participant
    /// cannot reuse work admitted under an older value.
    pub membership_epoch: u64,
    /// Session-wide disclosure generation. Any audience change invalidates
    /// continuation/context reuse until re-projected.
    pub audience_epoch: u64,
    pub owner: bool,
}

fn state(tx: &Transaction<'_>, session: &str) -> Result<(u64, u64)> {
    db(tx.query_row(
        "SELECT audience_epoch,management_revision
         FROM session_participation_state WHERE session_id=?1",
        [session],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))
}

pub(crate) fn evidence_in(
    connection: &rusqlite::Connection,
    session: &str,
    actor: &PrincipalId,
) -> Result<SessionParticipationEvidence> {
    let _ = session_in(connection, session)?;
    let (audience_epoch, _management_revision): (u64, u64) = db(connection.query_row(
        "SELECT audience_epoch,management_revision
         FROM session_participation_state WHERE session_id=?1",
        [session],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;

    if principal_is_session_owner(connection, session, actor)? {
        let owner_user = session_owner_user(connection, session)?;
        let security_epoch: u64 = db(connection.query_row(
            "SELECT security_epoch FROM users WHERE user_id=?1 AND status='active'",
            [&owner_user],
            |row| row.get(0),
        ))?;
        if security_epoch == 0 {
            return Err(Error::Denied("Session owner security epoch is invalid"));
        }
        return Ok(SessionParticipationEvidence {
            session_id: session.to_owned(),
            actor: actor.clone(),
            permissions: SessionParticipationPermissions::ALL,
            membership_epoch: security_epoch,
            audience_epoch,
            owner: true,
        });
    }

    let user = user_for_principal(connection, actor)?.ok_or(Error::Denied(
        "Session participant identity is inactive or unknown",
    ))?;
    let row: Option<(i64, i64, i64, i64, i64, String, u64)> = db(connection
        .query_row(
            "SELECT can_view,can_contribute,can_submit,can_control,can_manage,status,revision
             FROM session_participation_grants
             WHERE session_id=?1 AND user_id=?2",
            params![session, user],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional())?;
    let Some((view, contribute, submit, control, manage, status, revision)) = row else {
        return Err(Error::Denied("Session participation is not granted"));
    };
    if status != "active" || revision == 0 {
        return Err(Error::Denied("Session participation is revoked"));
    }
    let permissions = SessionParticipationPermissions {
        view: view != 0,
        contribute: contribute != 0,
        submit: submit != 0,
        control: control != 0,
        manage: manage != 0,
    }
    .validate()?;
    Ok(SessionParticipationEvidence {
        session_id: session.to_owned(),
        actor: actor.clone(),
        permissions,
        membership_epoch: revision,
        audience_epoch,
        owner: false,
    })
}

pub(crate) fn authorize_in(
    connection: &rusqlite::Connection,
    actor: &PrincipalId,
    session: &str,
    action: SessionParticipationAction,
) -> Result<SessionParticipationEvidence> {
    let evidence = evidence_in(connection, session, actor)?;
    if !evidence.permissions.allows(action) {
        return Err(Error::Denied("Session participation action is not granted"));
    }
    Ok(evidence)
}

impl SessionStore {
    /// Create a privacy-isolated approved-context Session in the source
    /// Session's existing personal Workspace. Only the explicitly approved text
    /// crosses the boundary. Source Turns, context artifacts/provider
    /// continuation, credentials, tool outputs and source binding hints are not
    /// copied. The caller supplies a fresh destination binding explicitly.
    #[allow(clippy::too_many_arguments)]
    pub fn create_approved_context_session(
        &self,
        owner: &PrincipalId,
        source_session: &str,
        source_branch: &str,
        expected_source_head: &str,
        expected_source_revision: u64,
        title: &str,
        binding: SessionBinding,
        approved_context: &str,
    ) -> Result<(SessionInfo, BranchInfo)> {
        crate::label(title, 160)?;
        crate::validate_binding(&binding)?;
        if approved_context.trim().is_empty()
            || approved_context.len() > aw_agent_core::limits::MAX_PROMPT_BYTES
            || approved_context.contains('\0')
        {
            return Err(Error::Limit(
                "approved context needs 1..131072 bounded UTF-8 bytes",
            ));
        }
        if expected_source_head.is_empty()
            || expected_source_head.len() > 256
            || expected_source_head.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid approved-context source head"));
        }

        let mut connection = self.connection()?;
        let tx =
            db(connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        if !principal_is_session_owner(&tx, source_session, owner)? {
            return Err(Error::Denied(
                "approved-context isolation must be created by the source Session owner",
            ));
        }
        let source = session_in(&tx, source_session)?;
        let source_branch_row = crate::branch_in(&tx, source_session, source_branch)?;
        if source_branch_row.id != source_branch
            || source_branch_row.head != expected_source_head
            || source_branch_row.revision != expected_source_revision
            || source_branch_row.active_run.is_some()
        {
            return Err(Error::Conflict(
                "approved-context source branch changed or is active; refresh the vetted snapshot",
            ));
        }

        let (destination, destination_branch) = crate::resources::create_in_workspace_tx(
            &tx,
            owner,
            title,
            &binding,
            &source.workspace_id,
            None,
        )?;
        let checkpoint = crate::id("n")?;
        let text_sha256 = format!("{:x}", Sha256::digest(approved_context.as_bytes()));
        crate::insert_node(
            &tx,
            &destination.id,
            &checkpoint,
            Some(&destination.root_node),
            "checkpoint",
            owner,
            serde_json::json!({
                "summary": approved_context,
                "approved_context": true,
                "source": {
                    "session": source_session,
                    "branch": source_branch_row.id,
                    "head": source_branch_row.head,
                    "revision": source_branch_row.revision,
                },
                "text_sha256": text_sha256,
            }),
        )?;
        let changed = db(tx.execute(
            "UPDATE branches
             SET head=?1,revision=1
             WHERE id=?2 AND session_id=?3 AND revision=0 AND head=?4",
            params![
                checkpoint,
                destination_branch.id,
                destination.id,
                destination.root_node
            ],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "approved-context destination branch changed during creation",
            ));
        }
        db(tx.execute(
            "UPDATE sessions SET updated_at=?1 WHERE id=?2",
            params![now(), destination.id],
        ))?;
        let txid = durability::tx_id()?;
        durability::ref_event(
            &tx,
            &destination_branch,
            &checkpoint,
            1,
            owner,
            "approved_context",
            Some(&checkpoint),
            &txid,
        )?;
        audit(
            &tx,
            source_session,
            owner,
            "session.approved_context.exported",
            serde_json::json!({
                "destination_session": destination.id,
                "source_branch": source_branch_row.id,
                "source_head": source_branch_row.head,
                "source_revision": source_branch_row.revision,
                "approved_text_sha256": text_sha256,
            }),
        )?;
        audit(
            &tx,
            &destination.id,
            owner,
            "session.approved_context.created",
            serde_json::json!({
                "source_session": source_session,
                "source_branch": source_branch_row.id,
                "source_head": source_branch_row.head,
                "source_revision": source_branch_row.revision,
                "approved_text_sha256": text_sha256,
            }),
        )?;
        durability::commit_event(
            &tx,
            &txid,
            "session.approved_context.created",
            "session",
            &destination.id,
            Some(owner),
            serde_json::json!({
                "workspace_id": source.workspace_id,
                "source_session": source_session,
                "source_branch": source_branch_row.id,
                "source_head": source_branch_row.head,
                "source_revision": source_branch_row.revision,
                "approved_text_sha256": text_sha256,
            }),
        )?;

        let destination = session_in(&tx, &destination.id)?;
        let destination_branch = crate::branch_in(&tx, &destination.id, &destination_branch.id)?;
        db(tx.commit())?;
        Ok((destination, destination_branch))
    }

    /// Current disclosure/model-context gate for a participant-origin paid
    /// Step or same-Turn Control admission. Participation authority and
    /// provider/model identity remain separate; both must be current.
    pub fn authorize_participant_model_context(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        identity: &aw_agent_core::InvocationIdentity,
        action: SessionParticipationAction,
    ) -> Result<()> {
        if identity.actor != *actor
            || !matches!(
                action,
                SessionParticipationAction::Submit | SessionParticipationAction::Control
            )
        {
            return Err(Error::Denied(
                "participant model context requires exact Submit/Control identity",
            ));
        }
        let connection = self.connection()?;
        let current = evidence_in(&connection, session, actor)?;
        if current.owner || !current.permissions.allows(action) {
            return Err(Error::Denied(
                "participant model context action is not currently authorized",
            ));
        }
        let branch = crate::branch_in(&connection, session, branch)?;
        crate::context::authorize_model_context_for_participation(
            &connection,
            session,
            &branch,
            identity,
            action,
        )
    }

    pub fn session_participation(
        &self,
        actor: &PrincipalId,
        session: &str,
    ) -> Result<SessionParticipationEvidence> {
        let connection = self.connection()?;
        evidence_in(&connection, session, actor)
    }

    pub fn authorize_session_participation(
        &self,
        actor: &PrincipalId,
        session: &str,
        action: SessionParticipationAction,
    ) -> Result<SessionParticipationEvidence> {
        let connection = self.connection()?;
        authorize_in(&connection, actor, session, action)
    }

    /// Set one participant's collaboration predicates. The requestor must hold
    /// current Manage authority. Resource ownership is unchanged.
    pub fn set_session_participation(
        &self,
        requestor: &PrincipalId,
        session: &str,
        participant: &PrincipalId,
        permissions: SessionParticipationPermissions,
    ) -> Result<SessionParticipationEvidence> {
        let permissions = permissions.validate()?;
        let mut connection = self.connection()?;
        let tx =
            db(connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let _ = session_in(&tx, session)?;

        let manager = evidence_in(&tx, session, requestor)?;
        if !manager.permissions.manage {
            return Err(Error::Denied(
                "Session participation changes require Manage authority",
            ));
        }
        if principal_is_session_owner(&tx, session, participant)? {
            return Err(Error::Denied(
                "Session owner participation is implicit and cannot be replaced",
            ));
        }
        let participant_user = ensure_user_for_principal(&tx, participant)?;
        let manager_user = user_for_principal(&tx, requestor)?
            .ok_or(Error::Denied("Session manager identity is inactive"))?;

        let current: Option<(i64, i64, i64, i64, i64, String, u64)> = db(tx
            .query_row(
                "SELECT can_view,can_contribute,can_submit,can_control,can_manage,status,revision
                 FROM session_participation_grants
                 WHERE session_id=?1 AND user_id=?2",
                params![session, participant_user],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional())?;
        let current_permissions =
            current
                .as_ref()
                .map_or(SessionParticipationPermissions::default(), |row| {
                    SessionParticipationPermissions {
                        view: row.5 == "active" && row.0 != 0,
                        contribute: row.5 == "active" && row.1 != 0,
                        submit: row.5 == "active" && row.2 != 0,
                        control: row.5 == "active" && row.3 != 0,
                        manage: row.5 == "active" && row.4 != 0,
                    }
                });
        if current_permissions == permissions {
            let revision = current.as_ref().map_or(1, |row| row.6);
            let (audience_epoch, _) = state(&tx, session)?;
            let evidence = SessionParticipationEvidence {
                session_id: session.to_owned(),
                actor: participant.clone(),
                permissions,
                membership_epoch: revision,
                audience_epoch,
                owner: false,
            };
            db(tx.commit())?;
            return Ok(evidence);
        }

        if !current_permissions.view && permissions.view {
            crate::context::authorize_dependencies_for_grant(&tx, session, participant)?;
        }

        let (audience_epoch, management_revision) = state(&tx, session)?;
        let next_revision = current
            .as_ref()
            .map_or(1, |row| row.6.checked_add(1).unwrap_or(0));
        if next_revision == 0 {
            return Err(Error::Limit("Session participant grant revision exhausted"));
        }
        let next_management = management_revision
            .checked_add(1)
            .ok_or(Error::Limit("Session participation revision exhausted"))?;
        let next_audience = if current_permissions.view != permissions.view {
            audience_epoch
                .checked_add(1)
                .ok_or(Error::Limit("Session audience epoch exhausted"))?
        } else {
            audience_epoch
        };
        let status = if permissions.is_empty() {
            "revoked"
        } else {
            "active"
        };
        let at = now();
        db(tx.execute(
            "INSERT INTO session_participation_grants(
                session_id,user_id,can_view,can_contribute,can_submit,can_control,can_manage,
                status,revision,granted_by_user_id,updated_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(session_id,user_id) DO UPDATE SET
                can_view=excluded.can_view,
                can_contribute=excluded.can_contribute,
                can_submit=excluded.can_submit,
                can_control=excluded.can_control,
                can_manage=excluded.can_manage,
                status=excluded.status,
                revision=excluded.revision,
                granted_by_user_id=excluded.granted_by_user_id,
                updated_at=excluded.updated_at",
            params![
                session,
                participant_user,
                i64::from(permissions.view),
                i64::from(permissions.contribute),
                i64::from(permissions.submit),
                i64::from(permissions.control),
                i64::from(permissions.manage),
                status,
                next_revision,
                manager_user,
                at,
            ],
        ))?;
        db(tx.execute(
            "UPDATE session_participation_state
             SET audience_epoch=?1,management_revision=?2,updated_at=?3
             WHERE session_id=?4",
            params![next_audience, next_management, at, session],
        ))?;
        audit(
            &tx,
            session,
            requestor,
            "session.participation.changed",
            serde_json::json!({
                "participant": participant.as_str(),
                "permissions": permissions,
                "membership_epoch": next_revision,
                "audience_epoch": next_audience,
                "management_revision": next_management,
            }),
        )?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "session.participation.changed",
            "session",
            session,
            Some(requestor),
            serde_json::json!({
                "session_id": session,
                "participant": participant.as_str(),
                "membership_epoch": next_revision,
                "audience_epoch": next_audience,
            }),
        )?;
        let evidence = SessionParticipationEvidence {
            session_id: session.to_owned(),
            actor: participant.clone(),
            permissions,
            membership_epoch: next_revision,
            audience_epoch: next_audience,
            owner: false,
        };
        db(tx.commit())?;
        Ok(evidence)
    }

    pub fn revoke_session_participation(
        &self,
        requestor: &PrincipalId,
        session: &str,
        participant: &PrincipalId,
    ) -> Result<SessionParticipationEvidence> {
        let before = self.session_participation(participant, session)?;
        self.set_session_participation(
            requestor,
            session,
            participant,
            SessionParticipationPermissions::default(),
        )?;
        // Return the immutable evidence that was revoked; callers can retain it
        // as factual provenance but must fail current authorization checks.
        Ok(before)
    }

    /// Revalidate an already-admitted participant receipt against the current
    /// grant and disclosure audience. This is called at queue claim/effects.
    pub fn revalidate_session_participation(
        &self,
        evidence: &SessionParticipationEvidence,
        action: SessionParticipationAction,
    ) -> Result<()> {
        let current =
            self.authorize_session_participation(&evidence.actor, &evidence.session_id, action)?;
        if current.membership_epoch != evidence.membership_epoch
            || current.audience_epoch != evidence.audience_epoch
            || current.owner != evidence.owner
        {
            return Err(Error::Denied(
                "Session participation/audience authority changed after admission",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_store_api::SessionBinding;
    use std::path::PathBuf;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[test]
    fn participation_is_independent_and_revocation_advances_audience_epoch() {
        let store = SessionStore::in_memory().unwrap();
        let alice = PrincipalId::new("internal:fixture:alice-share").unwrap();
        let bob = PrincipalId::new("internal:fixture:bob-share").unwrap();
        let (session, _) = store.create(&alice, "shared", binding()).unwrap();

        let owner = store
            .authorize_session_participation(
                &alice,
                &session.id,
                SessionParticipationAction::Manage,
            )
            .unwrap();
        assert!(owner.owner);
        assert_eq!(owner.permissions, SessionParticipationPermissions::ALL);

        let grant = store
            .set_session_participation(
                &alice,
                &session.id,
                &bob,
                SessionParticipationPermissions {
                    view: true,
                    submit: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(grant.permissions.submit);
        assert!(!grant.permissions.control);
        assert!(store
            .authorize_session_participation(&bob, &session.id, SessionParticipationAction::Submit,)
            .is_ok());
        assert!(
            store
                .authorize_session_participation(
                    &bob,
                    &session.id,
                    SessionParticipationAction::Control,
                )
                .is_err()
        );

        let old_membership = grant.membership_epoch;
        let old_audience = grant.audience_epoch;
        let revoked = store
            .revoke_session_participation(&alice, &session.id, &bob)
            .unwrap();
        assert_eq!(revoked.membership_epoch, old_membership);
        assert!(store.session_participation(&bob, &session.id).is_err());

        let regranted = store
            .set_session_participation(
                &alice,
                &session.id,
                &bob,
                SessionParticipationPermissions::VIEW_ONLY,
            )
            .unwrap();
        assert!(regranted.membership_epoch > old_membership);
        assert!(regranted.audience_epoch > old_audience);
        assert!(store
            .revalidate_session_participation(&grant, SessionParticipationAction::Submit)
            .is_err());
    }

    #[test]
    fn approved_context_session_is_same_workspace_but_private_history_and_provider_state_do_not_cross(
    ) {
        use aw_store_api::SessionBackend as _;

        let store = SessionStore::in_memory().unwrap();
        let alice = PrincipalId::new("internal:fixture:approved-owner").unwrap();
        let bob = PrincipalId::new("internal:fixture:approved-bob").unwrap();

        let private_binding = SessionBinding {
            working_directory: PathBuf::from("/tmp/private"),
            tool_root: PathBuf::from("/tmp/private"),
            system_prompt: "PRIVATE SYSTEM PROMPT".into(),
            profile_hint: "private-provider-account".into(),
            model_hint: "private-model".into(),
            destination_fingerprint: "a".repeat(64),
        };
        let (source, source_branch) = store
            .create(&alice, "private source", private_binding)
            .unwrap();
        let source_branch = store
            .checkpoint(
                &alice,
                &source.id,
                &source_branch.id,
                source_branch.revision,
                "PRIVATE HISTORY CHECKPOINT",
            )
            .unwrap();

        let public_binding = SessionBinding {
            working_directory: PathBuf::from("/tmp/shared"),
            tool_root: PathBuf::from("/tmp/shared"),
            system_prompt: "shared system".into(),
            profile_hint: "shared-account".into(),
            model_hint: "shared-model".into(),
            destination_fingerprint: "b".repeat(64),
        };
        let (approved, approved_branch) = store
            .create_approved_context_session(
                &alice,
                &source.id,
                &source_branch.id,
                &source_branch.head,
                source_branch.revision,
                "approved context",
                public_binding.clone(),
                "PUBLIC APPROVED CONTEXT ONLY",
            )
            .unwrap();

        assert_ne!(approved.id, source.id);
        assert_eq!(approved.workspace_id, source.workspace_id);
        assert_eq!(approved.owner, source.owner);
        assert_eq!(approved.binding.system_prompt, public_binding.system_prompt);
        assert_eq!(approved.binding.profile_hint, public_binding.profile_hint);
        assert_eq!(approved.binding.model_hint, public_binding.model_hint);
        assert_ne!(approved.binding.system_prompt, "PRIVATE SYSTEM PROMPT");
        assert_ne!(approved.binding.profile_hint, "private-provider-account");

        let owner_view = store
            .read_branch_view(&alice, &approved.id, &approved_branch.id)
            .unwrap();
        assert!(owner_view.history.turns.is_empty());
        assert!(owner_view.history.context_blocks.is_empty());
        assert_eq!(
            owner_view.history.summary.as_deref(),
            Some("PUBLIC APPROVED CONTEXT ONLY")
        );
        let serialized = serde_json::to_string(&owner_view.history).unwrap();
        assert!(!serialized.contains("PRIVATE HISTORY CHECKPOINT"));
        assert!(!serialized.contains("PRIVATE SYSTEM PROMPT"));
        assert!(!serialized.contains("private-provider-account"));
        assert!(!serialized.contains("private-model"));

        let source_binding = store
            .agent_binding(&alice, &source.id, &source_branch.id)
            .unwrap();
        let approved_binding = store
            .agent_binding(&alice, &approved.id, &approved_branch.id)
            .unwrap();
        assert!(source_binding.is_root_agent());
        assert!(!approved_binding.is_root_agent());
        assert_ne!(source_binding.agent_handle, approved_binding.agent_handle);

        store
            .set_session_participation(
                &alice,
                &approved.id,
                &bob,
                SessionParticipationPermissions::VIEW_ONLY,
            )
            .unwrap();
        let bob_view = store
            .read_branch_view(&bob, &approved.id, &approved_branch.id)
            .unwrap();
        assert_eq!(
            bob_view.history.summary.as_deref(),
            Some("PUBLIC APPROVED CONTEXT ONLY")
        );
        assert!(bob_view.history.turns.is_empty());
        assert!(bob_view.history.context_blocks.is_empty());
        let bob_serialized = serde_json::to_string(&bob_view.history).unwrap();
        assert!(!bob_serialized.contains("PRIVATE"));
    }

    #[test]
    fn legacy_append_membership_does_not_imply_submit_or_control() {
        let store = SessionStore::in_memory().unwrap();
        let alice = PrincipalId::new("internal:fixture:legacy-owner").unwrap();
        let bob = PrincipalId::new("internal:fixture:legacy-bob").unwrap();
        let (session, _) = store.create(&alice, "legacy", binding()).unwrap();
        store.grant(&alice, &session.id, &bob, Some(true)).unwrap();
        assert!(store.session(&bob, &session.id).is_ok());
        assert!(store
            .authorize_session_participation(&bob, &session.id, SessionParticipationAction::Submit,)
            .is_err());
    }
}
