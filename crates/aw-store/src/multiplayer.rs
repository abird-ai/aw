use super::*;
use aw_collab::DraftDocument;
pub use aw_collab::{
    candidate_digest, DraftGrant, DraftMode, DraftSnapshot, DraftState, DraftTarget,
    SealedCandidate,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_DRAFT_UPDATES: u64 = 16_384;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftPolicyInfo {
    pub session_id: String,
    pub mode: DraftMode,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftGrantInfo {
    pub session_id: String,
    pub user_id: String,
    pub grant: DraftGrant,
    pub scope_revision: u64,
    pub status: String,
}

fn mode_from_db(value: &str) -> Result<DraftMode> {
    match value {
        "off" => Ok(DraftMode::Off),
        "auto" => Ok(DraftMode::Auto),
        "on" => Ok(DraftMode::On),
        _ => Err(Error::Storage("invalid shared-draft mode in store")),
    }
}

fn state_from_db(value: &str) -> Result<DraftState> {
    match value {
        "active" => Ok(DraftState::Active),
        "sealed" => Ok(DraftState::Sealed),
        "admitted" => Ok(DraftState::Admitted),
        "rejected" => Ok(DraftState::Rejected),
        "abandoned" => Ok(DraftState::Abandoned),
        _ => Err(Error::Storage("invalid shared-draft state in store")),
    }
}

fn collab<T>(value: aw_collab::Result<T>) -> Result<T> {
    value.map_err(|error| match error {
        aw_collab::CollabError::UpdateTooLarge | aw_collab::CollabError::DraftTooLarge => {
            Error::Limit("shared draft exceeds configured bound")
        }
        aw_collab::CollabError::InvalidUpdate | aw_collab::CollabError::Materialize => {
            Error::Protocol("invalid shared-draft update")
        }
    })
}

fn owner_user(c: &Connection, session: &str, actor: &PrincipalId) -> Result<String> {
    if !principal_is_session_owner(c, session, actor)? {
        return Err(Error::Denied(
            "only Session owner manages shared-draft policy",
        ));
    }
    user_for_principal(c, actor)?.ok_or(Error::Denied("shared drafts require stable user identity"))
}

fn actor_user(c: &Connection, actor: &PrincipalId) -> Result<String> {
    user_for_principal(c, actor)?.ok_or(Error::Denied("shared drafts require stable user identity"))
}

fn policy_in(c: &Connection, session: &str) -> Result<DraftPolicyInfo> {
    let row: Option<(String, i64)> = db(c
        .query_row(
            "SELECT mode,revision FROM session_draft_policies WHERE session_id=?1",
            [session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    match row {
        Some((mode, revision)) => Ok(DraftPolicyInfo {
            session_id: session.to_owned(),
            mode: mode_from_db(&mode)?,
            revision: revision as u64,
        }),
        None => Ok(DraftPolicyInfo {
            session_id: session.to_owned(),
            mode: DraftMode::Off,
            revision: 0,
        }),
    }
}

fn grant_in(c: &Connection, session: &str, actor: &PrincipalId) -> Result<DraftGrantInfo> {
    if principal_is_session_owner(c, session, actor)? {
        let user = actor_user(c, actor)?;
        return Ok(DraftGrantInfo {
            session_id: session.to_owned(),
            user_id: user,
            grant: DraftGrant::owner(),
            scope_revision: policy_in(c, session)?.revision,
            status: "active".into(),
        });
    }
    let user = actor_user(c, actor)?;
    let row: Option<(i64, i64, i64, i64, i64, i64, String)> = db(c
        .query_row(
            "SELECT can_read,can_edit,can_submit,can_manage,presence_read,scope_revision,status FROM session_draft_grants WHERE session_id=?1 AND user_id=?2",
            params![session, user],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional())?;
    let Some((read, edit, submit, manage, presence, revision, status)) = row else {
        return Err(Error::Denied("shared draft unavailable or not permitted"));
    };
    if status != "active" {
        return Err(Error::Denied("shared draft unavailable or not permitted"));
    }
    Ok(DraftGrantInfo {
        session_id: session.to_owned(),
        user_id: user,
        grant: DraftGrant {
            can_read: read != 0,
            can_edit: edit != 0,
            can_submit: submit != 0,
            can_manage: manage != 0,
            presence_read: presence != 0,
        },
        scope_revision: revision as u64,
        status,
    })
}

pub(crate) fn draft_read_authorized_in(
    c: &Connection,
    session: &str,
    actor: &PrincipalId,
) -> Result<bool> {
    if principal_is_session_owner(c, session, actor)? {
        return Ok(true);
    }
    let Some(user) = user_for_principal(c, actor)? else {
        return Ok(false);
    };
    let policy = policy_in(c, session)?;
    if policy.mode == DraftMode::Off || policy.revision == 0 {
        return Ok(false);
    }
    db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM session_draft_grants
            WHERE session_id=?1 AND user_id=?2 AND status='active'
              AND can_read=1 AND scope_revision=?3
        )",
        params![session, user, policy.revision],
        |r| r.get(0),
    ))
}

fn current_submit_authorized_in(
    c: &Connection,
    session: &str,
    actor: &PrincipalId,
    scope_revision: u64,
) -> Result<bool> {
    let policy = policy_in(c, session)?;
    if policy.mode == DraftMode::Off || policy.revision != scope_revision {
        return Ok(false);
    }
    if principal_is_session_owner(c, session, actor)? {
        return Ok(true);
    }
    let Some(user) = user_for_principal(c, actor)? else {
        return Ok(false);
    };
    db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM session_draft_grants
            WHERE session_id=?1 AND user_id=?2 AND status='active'
              AND can_read=1 AND can_submit=1 AND scope_revision=?3
        )",
        params![session, user, scope_revision],
        |r| r.get(0),
    ))
}

pub(crate) fn draft_submit_authorized_in(
    c: &Connection,
    session: &str,
    actor: &PrincipalId,
) -> Result<bool> {
    let policy = policy_in(c, session)?;
    if policy.revision == 0 {
        return Ok(false);
    }
    current_submit_authorized_in(c, session, actor, policy.revision)
}

fn shared_submission_for_request_in(
    c: &Connection,
    actor: &PrincipalId,
    request: &str,
    session: &str,
    branch: &str,
    prompt: Option<&str>,
) -> Result<Option<String>> {
    let Some(actor_user) = user_for_principal(c, actor)? else {
        return Ok(None);
    };
    let row: Option<(String, String, i64, String, i64, Vec<u8>, String, String)> = db(c
        .query_row(
            "SELECT s.submission_id,s.status,d.audience_scope_revision,
                    s.target_head,s.target_branch_revision,s.candidate_text_utf8,
                    s.submitted_by_user_id,d.state
             FROM shared_draft_submissions s
             JOIN shared_drafts d
               ON d.draft_id=s.draft_id AND d.generation=s.generation
             WHERE s.prompt_request_id=?1 AND d.session_id=?2 AND d.branch_id=?3",
            params![request, session, branch],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .optional())?;
    let Some((
        submission_id,
        status,
        scope_revision,
        target_head,
        target_revision,
        candidate_text,
        submitter,
        draft_state,
    )) = row
    else {
        return Ok(None);
    };
    if submitter != actor_user
        || !matches!(status.as_str(), "sealed" | "admitted")
        || !matches!(draft_state.as_str(), "sealed" | "admitted")
        || !current_submit_authorized_in(c, session, actor, scope_revision as u64)?
    {
        return Ok(None);
    }
    let current = branch_in(c, session, branch)?;
    if current.head != target_head || current.revision != target_revision as u64 {
        return Ok(None);
    }
    if prompt.is_some_and(|value| value.as_bytes() != candidate_text.as_slice()) {
        return Ok(None);
    }
    Ok(Some(submission_id))
}

pub(crate) fn authorize_shared_request_in(
    c: &Connection,
    actor: &PrincipalId,
    request: &str,
    session: &str,
    branch: &str,
) -> Result<Option<String>> {
    shared_submission_for_request_in(c, actor, request, session, branch, None)
}

pub(crate) fn authorize_shared_receipt_in(
    c: &Connection,
    actor: &PrincipalId,
    receipt: u64,
    session: &str,
    branch: &str,
    prompt: Option<&str>,
) -> Result<Option<String>> {
    let request: Option<String> = db(c
        .query_row(
            "SELECT request_id FROM web_requests
             WHERE seq=?1 AND actor=?2 AND session_id=?3 AND branch_id=?4
               AND state='accepted'",
            params![receipt, actor.as_str(), session, branch],
            |r| r.get(0),
        )
        .optional())?;
    let Some(request) = request else {
        return Ok(None);
    };
    shared_submission_for_request_in(c, actor, &request, session, branch, prompt)
}

pub(crate) fn authorize_shared_run_in(
    c: &Connection,
    actor: &PrincipalId,
    run: &str,
    session: &str,
) -> Result<bool> {
    let Some(actor_user) = user_for_principal(c, actor)? else {
        return Ok(false);
    };
    db(c.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM web_requests w
            JOIN shared_draft_submissions s
              ON s.prompt_request_id=w.request_id
            JOIN shared_drafts d
              ON d.draft_id=s.draft_id AND d.generation=s.generation
            WHERE w.native_run_id=?1 AND w.actor=?2 AND w.session_id=?3
              AND s.submitted_by_user_id=?4 AND s.status='admitted'
              AND d.state='admitted'
        )",
        params![run, actor.as_str(), session, actor_user],
        |r| r.get(0),
    ))
}

fn require_read(c: &Connection, session: &str, actor: &PrincipalId) -> Result<DraftGrantInfo> {
    authorize(c, session, actor, false)?;
    let grant = grant_in(c, session, actor)?;
    if !grant.grant.can_read {
        return Err(Error::Denied("shared draft read permission required"));
    }
    Ok(grant)
}

fn require_edit(c: &Connection, session: &str, actor: &PrincipalId) -> Result<DraftGrantInfo> {
    let grant = require_read(c, session, actor)?;
    if !grant.grant.can_edit {
        return Err(Error::Denied("shared draft edit permission required"));
    }
    Ok(grant)
}

fn require_submit(c: &Connection, session: &str, actor: &PrincipalId) -> Result<DraftGrantInfo> {
    let grant = grant_in(c, session, actor)?;
    if !grant.grant.can_submit {
        return Err(Error::Denied("shared draft submit permission required"));
    }
    Ok(grant)
}

fn snapshot_row(c: &Connection, draft_id: &str, generation: u64) -> Result<DraftSnapshot> {
    let row: Option<(String, String, String, i64, i64, i64, i64, String, i64, Vec<u8>, Vec<u8>)> = db(c
        .query_row(
            "SELECT d.session_id,d.branch_id,d.state,d.document_epoch,d.accepted_revision,d.metadata_revision,d.audience_scope_revision,d.target_head,d.target_branch_revision,d.text_utf8,d.update_v1 FROM shared_drafts d WHERE d.draft_id=?1 AND d.generation=?2",
            params![draft_id, generation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)),
        )
        .optional())?;
    let Some((
        session,
        branch,
        state,
        epoch,
        accepted,
        metadata,
        audience,
        head,
        branch_revision,
        text,
        update,
    )) = row
    else {
        return Err(Error::Denied("shared draft unavailable or not permitted"));
    };
    let policy = policy_in(c, &session)?;
    if audience as u64 != policy.revision && state == "active" {
        return Err(Error::Conflict(
            "shared draft audience changed; reopen a new generation",
        ));
    }
    let text =
        String::from_utf8(text).map_err(|_| Error::Storage("shared draft text is not UTF-8"))?;
    Ok(DraftSnapshot {
        draft_id: draft_id.to_owned(),
        generation,
        document_epoch: epoch as u64,
        accepted_revision: accepted as u64,
        metadata_revision: metadata as u64,
        state: state_from_db(&state)?,
        mode: policy.mode,
        target: DraftTarget {
            session_id: session,
            branch_id: branch,
            head,
            branch_revision: branch_revision as u64,
        },
        text,
        update_v1: update,
    })
}

fn candidate_row(c: &Connection, submission_id: &str) -> Result<SealedCandidate> {
    let row: Option<(String,i64,i64,i64,String,i64,Vec<u8>,Vec<u8>,String,String,String,String,Option<String>)> = db(c
        .query_row(
            "SELECT s.draft_id,s.generation,s.accepted_revision,s.metadata_revision,d.session_id,s.target_branch_revision,s.candidate_text_utf8,s.candidate_update_v1,s.candidate_sha256,s.submitted_by_user_id,s.contributor_user_ids_json,s.status,s.prompt_request_id FROM shared_draft_submissions s JOIN shared_drafts d ON d.draft_id=s.draft_id AND d.generation=s.generation WHERE s.submission_id=?1",
            [submission_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?)),
        )
        .optional())?;
    let Some((
        draft_id,
        generation,
        accepted,
        metadata,
        session,
        branch_revision,
        text,
        update,
        digest,
        submitter,
        contributors_json,
        status,
        prompt_request_id,
    )) = row
    else {
        return Err(Error::Denied("shared draft submission unavailable"));
    };
    let target: (String,String)=db(c.query_row("SELECT d.branch_id,s.target_head FROM shared_draft_submissions s JOIN shared_drafts d ON d.draft_id=s.draft_id AND d.generation=s.generation WHERE s.submission_id=?1",[submission_id],|r|Ok((r.get(0)?,r.get(1)?))))?;
    let contributors: Vec<String> = serde_json::from_str(&contributors_json)
        .map_err(|_| Error::Storage("invalid shared draft contributor record"))?;
    Ok(SealedCandidate {
        submission_id: submission_id.to_owned(),
        draft_id,
        generation: generation as u64,
        accepted_revision: accepted as u64,
        metadata_revision: metadata as u64,
        target: DraftTarget {
            session_id: session,
            branch_id: target.0,
            head: target.1,
            branch_revision: branch_revision as u64,
        },
        text: String::from_utf8(text)
            .map_err(|_| Error::Storage("shared candidate is not UTF-8"))?,
        update_v1: update,
        digest_sha256: digest,
        submitted_by_user_id: submitter,
        contributor_user_ids: contributors,
        status,
        prompt_request_id,
    })
}

impl SessionStore {
    pub fn collaboration_user_id(&self, actor: &PrincipalId) -> Result<String> {
        let c = self.connection()?;
        actor_user(&c, actor)
    }

    pub fn draft_policy(&self, actor: &PrincipalId, session: &str) -> Result<DraftPolicyInfo> {
        let c = self.connection()?;
        authorize(&c, session, actor, false)?;
        policy_in(&c, session)
    }

    pub fn set_draft_mode(
        &self,
        owner: &PrincipalId,
        session: &str,
        mode: DraftMode,
    ) -> Result<DraftPolicyInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = owner_user(&tx, session, owner)?;
        let prior = policy_in(&tx, session)?;
        let revision = prior
            .revision
            .checked_add(1)
            .ok_or(Error::Limit("draft policy revision exhausted"))?;
        let at = now();
        db(tx.execute(
            "INSERT INTO session_draft_policies(session_id,mode,revision,updated_by_user_id,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(session_id) DO UPDATE SET mode=excluded.mode,revision=excluded.revision,updated_by_user_id=excluded.updated_by_user_id,updated_at=excluded.updated_at",
            params![session, mode.as_str(), revision, user, at],
        ))?;
        db(tx.execute("UPDATE session_draft_grants SET scope_revision=?1,updated_at=?2 WHERE session_id=?3 AND status='active'", params![revision,at,session]))?;
        // Any policy revision change rotates the active CRDT epoch. Keeping an
        // active row with an older audience/policy revision would make the room
        // permanently stale and could accidentally preserve an old disclosure
        // boundary across a mode change.
        db(tx.execute(
            "UPDATE shared_drafts SET state='abandoned',updated_at=?1 WHERE session_id=?2 AND state='active'",
            params![at, session],
        ))?;
        audit(
            &tx,
            session,
            owner,
            "draft.mode.changed",
            serde_json::json!({"mode":mode,"revision":revision}),
        )?;
        db(tx.commit())?;
        Ok(DraftPolicyInfo {
            session_id: session.to_owned(),
            mode,
            revision,
        })
    }

    pub fn set_draft_grant(
        &self,
        owner: &PrincipalId,
        session: &str,
        member: &PrincipalId,
        grant: Option<DraftGrant>,
    ) -> Result<Option<DraftGrantInfo>> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let owner_id = owner_user(&tx, session, owner)?;
        let member_id = ensure_user_for_principal(&tx, member)?;
        if owner_id == member_id {
            return Err(Error::Denied(
                "Session owner already has full shared-draft authority",
            ));
        }
        let prior = policy_in(&tx, session)?;
        let revision = prior
            .revision
            .checked_add(1)
            .ok_or(Error::Limit("draft audience revision exhausted"))?;
        let at = now();
        if prior.revision == 0 {
            db(tx.execute("INSERT INTO session_draft_policies(session_id,mode,revision,updated_by_user_id,updated_at) VALUES(?1,'off',?2,?3,?4)",params![session,revision,owner_id,at]))?;
        } else {
            db(tx.execute("UPDATE session_draft_policies SET revision=?1,updated_by_user_id=?2,updated_at=?3 WHERE session_id=?4",params![revision,owner_id,at,session]))?;
        }
        db(tx.execute("UPDATE session_draft_grants SET scope_revision=?1,updated_at=?2 WHERE session_id=?3 AND status='active'",params![revision,at,session]))?;
        let output = match grant {
            Some(grant) => {
                if (grant.can_edit || grant.can_submit || grant.can_manage || grant.presence_read)
                    && !grant.can_read
                {
                    return Err(Error::Denied(
                        "shared-draft actions require read permission",
                    ));
                }
                context::authorize_tree(&tx, session, owner)?;
                context::authorize_dependencies_for_grant(&tx, session, member)?;
                // Draft permissions are not generic Session membership. In
                // particular, can_submit must not manufacture members.can_append:
                // exact shared submission has its own sealed, request-bound
                // admission capability and ordinary fork/checkpoint APIs remain
                // governed by the pre-existing Session membership record.
                db(tx.execute("INSERT INTO session_draft_grants(session_id,user_id,can_read,can_edit,can_submit,can_manage,presence_read,scope_revision,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?9) ON CONFLICT(session_id,user_id) DO UPDATE SET can_read=excluded.can_read,can_edit=excluded.can_edit,can_submit=excluded.can_submit,can_manage=excluded.can_manage,presence_read=excluded.presence_read,scope_revision=excluded.scope_revision,status='active',updated_at=excluded.updated_at",params![session,member_id,i64::from(grant.can_read),i64::from(grant.can_edit),i64::from(grant.can_submit),i64::from(grant.can_manage),i64::from(grant.presence_read),revision,at]))?;
                Some(DraftGrantInfo {
                    session_id: session.to_owned(),
                    user_id: member_id.clone(),
                    grant,
                    scope_revision: revision,
                    status: "active".into(),
                })
            }
            None => {
                db(tx.execute("UPDATE session_draft_grants SET status='revoked',scope_revision=?1,updated_at=?2 WHERE session_id=?3 AND user_id=?4",params![revision,at,session,member_id]))?;
                None
            }
        };
        // A changed audience never inherits the old CRDT update history. The next
        // join creates a fresh document epoch/generation.
        db(tx.execute("UPDATE shared_drafts SET state='abandoned',updated_at=?1 WHERE session_id=?2 AND state='active'",params![at,session]))?;
        audit(
            &tx,
            session,
            owner,
            "draft.audience.changed",
            serde_json::json!({"member_user_id":member_id,"active":output.is_some(),"scope_revision":revision}),
        )?;
        db(tx.commit())?;
        Ok(output)
    }

    pub fn draft_grant(&self, actor: &PrincipalId, session: &str) -> Result<DraftGrantInfo> {
        let c = self.connection()?;
        grant_in(&c, session, actor)
    }

    pub fn draft_grants(&self, owner: &PrincipalId, session: &str) -> Result<Vec<DraftGrantInfo>> {
        let c = self.connection()?;
        owner_user(&c, session, owner)?;
        let mut query = db(c.prepare(
            "SELECT user_id,can_read,can_edit,can_submit,can_manage,presence_read,scope_revision,status
             FROM session_draft_grants
             WHERE session_id=?1 AND status='active'
             ORDER BY user_id",
        ))?;
        let rows = db(query.query_map([session], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
            ))
        }))?;
        let mut output = Vec::new();
        for row in rows {
            let (user_id, read, edit, submit, manage, presence, revision, status) = db(row)?;
            output.push(DraftGrantInfo {
                session_id: session.to_owned(),
                user_id,
                grant: DraftGrant {
                    can_read: read != 0,
                    can_edit: edit != 0,
                    can_submit: submit != 0,
                    can_manage: manage != 0,
                    presence_read: presence != 0,
                },
                scope_revision: revision as u64,
                status,
            });
        }
        Ok(output)
    }

    /// Explicit multiplayer discovery. Ordinary Session inventory remains based
    /// on Session membership; draft sharing is visible only through this API.
    pub fn draft_shared_sessions(&self, actor: &PrincipalId) -> Result<Vec<SessionInfo>> {
        let c = self.connection()?;
        let Some(user) = user_for_principal(&c, actor)? else {
            return Ok(Vec::new());
        };
        let mut query = db(c.prepare(
            "SELECT g.session_id
             FROM session_draft_grants g
             JOIN session_draft_policies p ON p.session_id=g.session_id
             WHERE g.user_id=?1 AND g.status='active' AND g.can_read=1
               AND g.scope_revision=p.revision AND p.mode<>'off'
             ORDER BY g.updated_at DESC,g.session_id
             LIMIT 200",
        ))?;
        let rows = db(query.query_map([user], |row| row.get::<_, String>(0)))?;
        let ids = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        ids.into_iter()
            .filter_map(
                |session| match context::authorize_tree(&c, &session, actor) {
                    Ok(()) => Some(session_in(&c, &session)),
                    Err(Error::Denied(_)) => None,
                    Err(error) => Some(Err(error)),
                },
            )
            .collect()
    }

    pub fn open_shared_draft(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        activate_auto: bool,
    ) -> Result<DraftSnapshot> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let grant = require_edit(&tx, session, actor)?;
        let policy = policy_in(&tx, session)?;
        if policy.mode == DraftMode::Off {
            return Err(Error::Denied("multiplayer is Off for this Session"));
        }
        if let Some((draft_id, generation)) = db(tx
            .query_row("SELECT draft_id,generation FROM shared_drafts WHERE session_id=?1 AND branch_id=?2 AND state='active'",params![session,branch],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))
            .optional())? {
            let out = snapshot_row(&tx,&draft_id,generation as u64)?;
            db(tx.commit())?;
            return Ok(out);
        }
        if policy.mode == DraftMode::Auto && !activate_auto {
            return Err(Error::Conflict(
                "Auto multiplayer is eligible but no shared draft is active",
            ));
        }
        let branch_row = branch_in(&tx, session, branch)?;
        let previous: Option<(String,i64,i64)> = db(tx
            .query_row("SELECT draft_id,generation,document_epoch FROM shared_drafts WHERE session_id=?1 AND branch_id=?2 ORDER BY generation DESC LIMIT 1",params![session,branch],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
            .optional())?;
        let (draft_id, generation, epoch) = match previous {
            Some((id, g, e)) => (
                id,
                (g as u64)
                    .checked_add(1)
                    .ok_or(Error::Limit("draft generation exhausted"))?,
                (e as u64)
                    .checked_add(1)
                    .ok_or(Error::Limit("draft epoch exhausted"))?,
            ),
            None => (id("draft")?, 1, 1),
        };
        let doc = DraftDocument::new();
        let update = doc.encode_full_v1();
        let at = now();
        db(tx.execute("INSERT INTO shared_drafts(draft_id,generation,session_id,branch_id,state,document_epoch,accepted_revision,metadata_revision,audience_scope_revision,target_head,target_branch_revision,update_v1,text_utf8,created_by_user_id,created_at,updated_at) VALUES(?1,?2,?3,?4,'active',?5,0,1,?6,?7,?8,?9,X'',?10,?11,?11)",params![draft_id,generation,session,branch,epoch,policy.revision,branch_row.head,branch_row.revision,update,grant.user_id,at]))?;
        let out = snapshot_row(&tx, &draft_id, generation)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn shared_draft_snapshot(
        &self,
        actor: &PrincipalId,
        draft_id: &str,
        generation: u64,
    ) -> Result<DraftSnapshot> {
        let c = self.connection()?;
        let out = snapshot_row(&c, draft_id, generation)?;
        require_read(&c, &out.target.session_id, actor)?;
        Ok(out)
    }

    pub fn apply_shared_draft_update(
        &self,
        actor: &PrincipalId,
        draft_id: &str,
        generation: u64,
        document_epoch: u64,
        update_id: &str,
        update_v1: &[u8],
    ) -> Result<DraftSnapshot> {
        if update_id.is_empty()
            || update_id.len() > 96
            || !update_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(Error::Denied("invalid shared-draft update ID"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = snapshot_row(&tx, draft_id, generation)?;
        let grant = require_edit(&tx, &current.target.session_id, actor)?;
        if current.state != DraftState::Active {
            return Err(Error::Conflict("GenerationSealed"));
        }
        if current.document_epoch != document_epoch {
            return Err(Error::Conflict("StaleEpoch"));
        }
        if let Some(revision)=db(tx.query_row("SELECT accepted_revision FROM shared_draft_updates WHERE draft_id=?1 AND generation=?2 AND update_id=?3",params![draft_id,generation,update_id],|r|r.get::<_,i64>(0)).optional())? {
            let mut out=current;
            out.accepted_revision=revision as u64;
            db(tx.commit())?;
            return Ok(out);
        }
        if current.accepted_revision >= MAX_DRAFT_UPDATES {
            return Err(Error::Limit("shared draft update limit reached"));
        }
        let doc = collab(DraftDocument::from_update_v1(&current.update_v1))?;
        collab(doc.apply_update_v1(update_v1))?;
        let text = doc.text();
        if text.len() > aw_agent_core::limits::MAX_PROMPT_BYTES {
            return Err(Error::Limit("shared prompt exceeds prompt byte limit"));
        }
        let full = doc.encode_full_v1();
        let revision = current
            .accepted_revision
            .checked_add(1)
            .ok_or(Error::Limit("draft revision exhausted"))?;
        let at = now();
        db(tx.execute("UPDATE shared_drafts SET accepted_revision=?1,update_v1=?2,text_utf8=?3,updated_at=?4 WHERE draft_id=?5 AND generation=?6 AND state='active' AND document_epoch=?7",params![revision,full,text.as_bytes(),at,draft_id,generation,document_epoch]))?;
        db(tx.execute("INSERT INTO shared_draft_updates(draft_id,generation,accepted_revision,update_id,contributor_user_id,update_v1,accepted_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![draft_id,generation,revision,update_id,grant.user_id,update_v1,at]))?;
        let out = snapshot_row(&tx, draft_id, generation)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn seal_shared_draft(
        &self,
        actor: &PrincipalId,
        draft_id: &str,
        generation: u64,
        document_epoch: u64,
        accepted_revision: u64,
        metadata_revision: u64,
        digest_sha256: &str,
    ) -> Result<SealedCandidate> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        if let Some(existing)=db(tx.query_row("SELECT submission_id FROM shared_draft_submissions WHERE draft_id=?1 AND generation=?2",params![draft_id,generation],|r|r.get::<_,String>(0)).optional())? {
            let out=candidate_row(&tx,&existing)?;
            require_read(&tx,&out.target.session_id,actor)?;
            db(tx.commit())?;
            return Ok(out);
        }
        let current = snapshot_row(&tx, draft_id, generation)?;
        let grant = require_submit(&tx, &current.target.session_id, actor)?;
        if current.state != DraftState::Active {
            return Err(Error::Conflict("GenerationSealed"));
        }
        if current.document_epoch != document_epoch {
            return Err(Error::Conflict("StaleEpoch"));
        }
        if current.accepted_revision != accepted_revision
            || current.metadata_revision != metadata_revision
        {
            return Err(Error::Conflict("DraftChanged"));
        }
        if current.text.trim().is_empty() {
            return Err(Error::Denied("shared prompt is empty"));
        }
        let branch = branch_in(&tx, &current.target.session_id, &current.target.branch_id)?;
        if branch.head != current.target.head || branch.revision != current.target.branch_revision {
            return Err(Error::Conflict("HeadMoved"));
        }
        let digest = candidate_digest(&current.text, &current.target, current.metadata_revision);
        if digest != digest_sha256 {
            return Err(Error::Conflict("DraftChanged"));
        }
        let mut contributors = BTreeSet::new();
        {
            let mut q=db(tx.prepare("SELECT DISTINCT contributor_user_id FROM shared_draft_updates WHERE draft_id=?1 AND generation=?2 ORDER BY contributor_user_id"))?;
            let rows = db(q.query_map(params![draft_id, generation], |r| r.get::<_, String>(0)))?;
            for row in rows {
                contributors.insert(db(row)?);
            }
        }
        if contributors.is_empty() {
            contributors.insert(grant.user_id.clone());
        }
        let contributors = contributors.into_iter().collect::<Vec<_>>();
        let submission_id = id("sub")?;
        let at = now();
        db(tx.execute("UPDATE shared_drafts SET state='sealed',updated_at=?1 WHERE draft_id=?2 AND generation=?3 AND state='active'",params![at,draft_id,generation]))?;
        db(tx.execute("INSERT INTO shared_draft_submissions(submission_id,draft_id,generation,accepted_revision,metadata_revision,target_head,target_branch_revision,candidate_text_utf8,candidate_update_v1,candidate_sha256,submitted_by_user_id,contributor_user_ids_json,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'sealed',?13,?13)",params![submission_id,draft_id,generation,accepted_revision,metadata_revision,current.target.head,current.target.branch_revision,current.text.as_bytes(),current.update_v1,digest,grant.user_id,json(&contributors)?,at]))?;
        audit(
            &tx,
            &current.target.session_id,
            actor,
            "draft.sealed",
            serde_json::json!({"submission_id":submission_id,"draft_id":draft_id,"generation":generation,"candidate_sha256":digest,"contributors":contributors}),
        )?;
        let out = candidate_row(&tx, &submission_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn bind_shared_draft_prompt_request(
        &self,
        actor: &PrincipalId,
        submission_id: &str,
        request_id: &str,
    ) -> Result<SealedCandidate> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let candidate = candidate_row(&tx, submission_id)?;
        require_submit(&tx, &candidate.target.session_id, actor)?;
        let user = actor_user(&tx, actor)?;
        if candidate.submitted_by_user_id != user {
            return Err(Error::Denied(
                "only the sealing submitter may execute this shared candidate",
            ));
        }
        if let Some(existing) = candidate.prompt_request_id.as_deref() {
            if existing != request_id {
                return Err(Error::Conflict(
                    "shared submission already bound to another request",
                ));
            }
        } else {
            db(tx.execute("UPDATE shared_draft_submissions SET prompt_request_id=?1,updated_at=?2 WHERE submission_id=?3 AND prompt_request_id IS NULL",params![request_id,now(),submission_id]))?;
        }
        let out = candidate_row(&tx, submission_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn mark_shared_draft_rejected(
        &self,
        submission_id: &str,
        code: &str,
    ) -> Result<SealedCandidate> {
        if code.len() > 64
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(Error::Denied("invalid shared submission rejection code"));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let candidate = candidate_row(&tx, submission_id)?;
        let at = now();
        db(tx.execute("UPDATE shared_draft_submissions SET status='rejected',rejection_code=?1,updated_at=?2 WHERE submission_id=?3 AND status='sealed'",params![code,at,submission_id]))?;
        db(tx.execute("UPDATE shared_drafts SET state='rejected',updated_at=?1 WHERE draft_id=?2 AND generation=?3 AND state='sealed'",params![at,candidate.draft_id,candidate.generation]))?;
        let out = candidate_row(&tx, submission_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn shared_draft_submission(
        &self,
        actor: &PrincipalId,
        submission_id: &str,
    ) -> Result<SealedCandidate> {
        let c = self.connection()?;
        let out = candidate_row(&c, submission_id)?;
        require_read(&c, &out.target.session_id, actor)?;
        Ok(out)
    }

    pub fn recover_shared_draft(
        &self,
        actor: &PrincipalId,
        submission_id: &str,
    ) -> Result<DraftSnapshot> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let candidate = candidate_row(&tx, submission_id)?;
        let grant = require_edit(&tx, &candidate.target.session_id, actor)?;
        let policy = policy_in(&tx, &candidate.target.session_id)?;
        if policy.mode == DraftMode::Off {
            return Err(Error::Denied("multiplayer is Off for this Session"));
        }
        if db(tx.query_row("SELECT EXISTS(SELECT 1 FROM shared_drafts WHERE session_id=?1 AND branch_id=?2 AND state='active')",params![candidate.target.session_id,candidate.target.branch_id],|r|r.get::<_,bool>(0)))? {return Err(Error::Conflict("an active shared draft already exists"));}
        let branch = branch_in(
            &tx,
            &candidate.target.session_id,
            &candidate.target.branch_id,
        )?;
        let prior = snapshot_row(&tx, &candidate.draft_id, candidate.generation)?;
        let generation = prior
            .generation
            .checked_add(1)
            .ok_or(Error::Limit("draft generation exhausted"))?;
        let epoch = prior
            .document_epoch
            .checked_add(1)
            .ok_or(Error::Limit("draft epoch exhausted"))?;
        let doc = DraftDocument::new();
        let _ = collab(doc.replace_text(&candidate.text))?;
        let full = doc.encode_full_v1();
        let at = now();
        db(tx.execute("INSERT INTO shared_drafts(draft_id,generation,session_id,branch_id,state,document_epoch,accepted_revision,metadata_revision,audience_scope_revision,target_head,target_branch_revision,update_v1,text_utf8,created_by_user_id,created_at,updated_at) VALUES(?1,?2,?3,?4,'active',?5,0,1,?6,?7,?8,?9,?10,?11,?12,?12)",params![candidate.draft_id,generation,candidate.target.session_id,candidate.target.branch_id,epoch,policy.revision,branch.head,branch.revision,full,candidate.text.as_bytes(),grant.user_id,at]))?;
        let out = snapshot_row(&tx, &candidate.draft_id, generation)?;
        db(tx.commit())?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::PrincipalId;
    use std::path::PathBuf;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: "system".into(),
            profile_hint: "p".into(),
            model_hint: "m".into(),
            destination_fingerprint: "a".repeat(64),
        }
    }

    fn setup() -> (
        SessionStore,
        PrincipalId,
        PrincipalId,
        SessionInfo,
        BranchInfo,
    ) {
        let store = SessionStore::in_memory().unwrap();
        let owner = PrincipalId::new("internal:team:alice").unwrap();
        let bob = PrincipalId::new("internal:team:bob").unwrap();
        let (s, b) = store.create(&owner, "shared", binding()).unwrap();
        (store, owner, bob, s, b)
    }

    #[test]
    fn off_is_allocation_free_until_explicitly_enabled() {
        let (store, owner, _, s, b) = setup();
        assert_eq!(
            store.draft_policy(&owner, &s.id).unwrap().mode,
            DraftMode::Off
        );
        assert!(matches!(
            store.open_shared_draft(&owner, &s.id, &b.id, false),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn explicit_grant_update_seal_and_duplicate_send_identity() {
        let (store, owner, bob, s, b) = setup();
        store
            .set_draft_grant(&owner, &s.id, &bob, Some(DraftGrant::editor_submitter()))
            .unwrap();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        let opened = store.open_shared_draft(&bob, &s.id, &b.id, false).unwrap();
        let client = DraftDocument::new();
        let update = client.replace_text("hello 🙂").unwrap();
        let changed = store
            .apply_shared_draft_update(
                &bob,
                &opened.draft_id,
                opened.generation,
                opened.document_epoch,
                "u1",
                &update,
            )
            .unwrap();
        assert_eq!(changed.text, "hello 🙂");
        let duplicate = store
            .apply_shared_draft_update(
                &bob,
                &opened.draft_id,
                opened.generation,
                opened.document_epoch,
                "u1",
                &update,
            )
            .unwrap();
        assert_eq!(duplicate.accepted_revision, changed.accepted_revision);
        let digest = candidate_digest(&changed.text, &changed.target, changed.metadata_revision);
        let sealed = store
            .seal_shared_draft(
                &bob,
                &changed.draft_id,
                changed.generation,
                changed.document_epoch,
                changed.accepted_revision,
                changed.metadata_revision,
                &digest,
            )
            .unwrap();
        let again = store
            .seal_shared_draft(
                &owner,
                &changed.draft_id,
                changed.generation,
                changed.document_epoch,
                changed.accepted_revision,
                changed.metadata_revision,
                &digest,
            )
            .unwrap();
        assert_eq!(sealed.submission_id, again.submission_id);
        assert!(matches!(
            store.apply_shared_draft_update(
                &bob,
                &changed.draft_id,
                changed.generation,
                changed.document_epoch,
                "late",
                &update
            ),
            Err(Error::Conflict("GenerationSealed"))
        ));
    }

    #[test]
    fn audience_change_abandons_old_epoch_and_revocation_blocks_access() {
        let (store, owner, bob, s, b) = setup();
        store
            .set_draft_grant(&owner, &s.id, &bob, Some(DraftGrant::editor_submitter()))
            .unwrap();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        let opened = store.open_shared_draft(&bob, &s.id, &b.id, false).unwrap();
        store.set_draft_grant(&owner, &s.id, &bob, None).unwrap();
        assert!(store
            .shared_draft_snapshot(&bob, &opened.draft_id, opened.generation)
            .is_err());
        let owner_view = store
            .shared_draft_snapshot(&owner, &opened.draft_id, opened.generation)
            .unwrap();
        assert_eq!(owner_view.state, DraftState::Abandoned);
    }

    #[test]
    fn stale_revision_or_head_refuses_exact_seal() {
        let (store, owner, _, s, b) = setup();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        let opened = store
            .open_shared_draft(&owner, &s.id, &b.id, false)
            .unwrap();
        let doc = DraftDocument::new();
        let update = doc.replace_text("one").unwrap();
        let changed = store
            .apply_shared_draft_update(
                &owner,
                &opened.draft_id,
                opened.generation,
                opened.document_epoch,
                "u1",
                &update,
            )
            .unwrap();
        let digest = candidate_digest(&changed.text, &changed.target, changed.metadata_revision);
        assert!(matches!(
            store.seal_shared_draft(
                &owner,
                &changed.draft_id,
                changed.generation,
                changed.document_epoch,
                0,
                changed.metadata_revision,
                &digest
            ),
            Err(Error::Conflict("DraftChanged"))
        ));
        store
            .checkpoint(&owner, &s.id, &b.id, b.revision, "move")
            .unwrap();
        assert!(matches!(
            store.seal_shared_draft(
                &owner,
                &changed.draft_id,
                changed.generation,
                changed.document_epoch,
                changed.accepted_revision,
                changed.metadata_revision,
                &digest
            ),
            Err(Error::Conflict("HeadMoved"))
        ));
    }

    #[test]
    fn rejected_candidate_can_recover_into_fresh_document_epoch() {
        let (store, owner, _, s, b) = setup();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        let opened = store
            .open_shared_draft(&owner, &s.id, &b.id, false)
            .unwrap();
        let doc = DraftDocument::new();
        let update = doc.replace_text("recover me").unwrap();
        let changed = store
            .apply_shared_draft_update(
                &owner,
                &opened.draft_id,
                opened.generation,
                opened.document_epoch,
                "u1",
                &update,
            )
            .unwrap();
        let digest = candidate_digest(&changed.text, &changed.target, changed.metadata_revision);
        let sealed = store
            .seal_shared_draft(
                &owner,
                &changed.draft_id,
                changed.generation,
                changed.document_epoch,
                changed.accepted_revision,
                changed.metadata_revision,
                &digest,
            )
            .unwrap();
        store
            .mark_shared_draft_rejected(&sealed.submission_id, "policy")
            .unwrap();
        let recovered = store
            .recover_shared_draft(&owner, &sealed.submission_id)
            .unwrap();
        assert_eq!(recovered.text, "recover me");
        assert!(recovered.document_epoch > changed.document_epoch);
        assert_eq!(recovered.accepted_revision, 0);
    }

    #[test]
    fn draft_submit_grant_does_not_create_generic_session_append() {
        let (store, owner, bob, s, b) = setup();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        store
            .set_draft_grant(&owner, &s.id, &bob, Some(DraftGrant::editor_submitter()))
            .unwrap();

        assert!(!store.branches(&bob, &s.id).unwrap().is_empty());
        assert!(matches!(
            store.checkpoint(
                &bob,
                &s.id,
                &b.id,
                b.revision,
                "draft submit is not generic append"
            ),
            Err(Error::Denied(_))
        ));
        assert!(matches!(
            store.fork(&bob, &s.id, "not-authorized", &b.head),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn draft_revocation_preserves_separate_context_read_release() {
        let (store, owner, bob, s, b) = setup();
        store
            .context_grant_read(&owner, &s.id, &bob, true, vec!["account:summary".into()])
            .unwrap();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        store
            .set_draft_grant(&owner, &s.id, &bob, Some(DraftGrant::editor_submitter()))
            .unwrap();
        store.set_draft_grant(&owner, &s.id, &bob, None).unwrap();

        assert!(!store.branches(&bob, &s.id).unwrap().is_empty());
        assert!(matches!(
            store.checkpoint(
                &bob,
                &s.id,
                &b.id,
                b.revision,
                "context read stays read-only"
            ),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn draft_revocation_preserves_preexisting_append_membership() {
        let (store, owner, bob, s, b) = setup();
        store.grant(&owner, &s.id, &bob, Some(true)).unwrap();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        store
            .set_draft_grant(&owner, &s.id, &bob, Some(DraftGrant::editor_submitter()))
            .unwrap();
        store.set_draft_grant(&owner, &s.id, &bob, None).unwrap();

        store
            .checkpoint(
                &bob,
                &s.id,
                &b.id,
                b.revision,
                "pre-existing Session append survives draft revoke",
            )
            .unwrap();
    }

    #[test]
    fn mode_change_rotates_active_generation_instead_of_leaving_stale_room() {
        let (store, owner, _, s, b) = setup();
        store.set_draft_mode(&owner, &s.id, DraftMode::On).unwrap();
        let first = store
            .open_shared_draft(&owner, &s.id, &b.id, false)
            .unwrap();

        store
            .set_draft_mode(&owner, &s.id, DraftMode::Auto)
            .unwrap();

        let old = store
            .shared_draft_snapshot(&owner, &first.draft_id, first.generation)
            .unwrap();
        assert_eq!(old.state, DraftState::Abandoned);
        assert!(matches!(
            store.open_shared_draft(&owner, &s.id, &b.id, false),
            Err(Error::Conflict(_))
        ));
        let next = store.open_shared_draft(&owner, &s.id, &b.id, true).unwrap();
        assert!(next.generation > first.generation);
        assert!(next.document_epoch > first.document_epoch);
        assert_eq!(next.mode, DraftMode::Auto);
    }
}
