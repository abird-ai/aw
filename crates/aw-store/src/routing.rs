use super::*;
use aw_agent_core::{
    InvocationContext, InvocationIdentity, InvocationPurpose, JournalFuture, RoutingInput,
    RoutingJournal, RoutingResult,
};
use aw_routing::{
    strategy_sha256, target as strategy_target, validate_strategy, RoutingMode,
    RoutingStrategyDefinition,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_ROUTING_STRATEGIES: i64 = 1024;
const MAX_ROUTING_REVISIONS: i64 = 4096;
const MAX_ROUTING_OPERATIONS: i64 = 100_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingStrategyRevisionInfo {
    pub strategy_id: String,
    pub revision_id: String,
    pub revision_no: u64,
    pub name: String,
    pub owner_user_id: String,
    pub definition_sha256: String,
    pub definition: RoutingStrategyDefinition,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingBindingScope {
    User,
    Session,
}
impl RoutingBindingScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Session => "session",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingBindingInfo {
    pub id: String,
    pub strategy_id: String,
    pub revision_id: String,
    pub scope: RoutingBindingScope,
    pub scope_user_id: Option<String>,
    pub scope_session_id: Option<String>,
    pub role: String,
    pub revision: u64,
    pub status: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingSelectionSource {
    Fixed,
    Affinity,
    Classifier,
}
impl RoutingSelectionSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Affinity => "affinity",
            Self::Classifier => "classifier",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingOperationInfo {
    pub id: String,
    pub strategy_id: String,
    pub revision_id: String,
    pub actor_user_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub branch_head: String,
    pub branch_revision: u64,
    pub role: String,
    pub lineage: String,
    pub mode: RoutingMode,
    pub selection_source: RoutingSelectionSource,
    pub input_sha256: String,
    pub candidate_ids_sha256: String,
    pub state: String,
    pub selected_target_id: Option<String>,
    pub working_identity: Option<InvocationIdentity>,
    pub consumed_at: Option<i64>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingAttemptInfo {
    pub id: String,
    pub operation_id: String,
    pub attempt_no: u32,
    pub classifier_identity: InvocationIdentity,
    pub state: String,
    pub metadata: Option<aw_agent_core::CompletionMeta>,
    pub selected_target_id: Option<String>,
    pub error: Option<String>,
    pub owner_process_id: String,
    pub owner_store_generation: u64,
    pub started_at: i64,
    pub settled_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingAffinityInfo {
    pub actor_user_id: String,
    pub session_id: String,
    pub branch_id: String,
    pub role: String,
    pub lineage: String,
    pub strategy_revision_id: String,
    pub target_id: String,
    pub established_operation_id: String,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingResolution {
    pub strategy: RoutingStrategyRevisionInfo,
    pub binding: Option<RoutingBindingInfo>,
}

fn routing_error<T>(value: aw_routing::Result<T>) -> Result<T> {
    value.map_err(|error| match error {
        aw_routing::RoutingError::EvidenceLimit => {
            Error::Limit("routing evidence exceeds configured bound")
        }
        aw_routing::RoutingError::UnknownCandidate => {
            Error::Denied("routing target is not a configured candidate")
        }
        aw_routing::RoutingError::Strategy(_) | aw_routing::RoutingError::InvalidProposal => {
            Error::Config(error.to_string())
        }
    })
}

fn mode_str(mode: RoutingMode) -> &'static str {
    match mode {
        RoutingMode::Fixed => "fixed",
        RoutingMode::RouteOnce => "route_once",
        RoutingMode::RouteEachPrompt => "route_each_prompt",
    }
}
fn mode_from(raw: &str) -> Result<RoutingMode> {
    match raw {
        "fixed" => Ok(RoutingMode::Fixed),
        "route_once" => Ok(RoutingMode::RouteOnce),
        "route_each_prompt" => Ok(RoutingMode::RouteEachPrompt),
        _ => Err(Error::Storage("invalid routing mode")),
    }
}
fn source_from(raw: &str) -> Result<RoutingSelectionSource> {
    match raw {
        "fixed" => Ok(RoutingSelectionSource::Fixed),
        "affinity" => Ok(RoutingSelectionSource::Affinity),
        "classifier" => Ok(RoutingSelectionSource::Classifier),
        _ => Err(Error::Storage("invalid routing selection source")),
    }
}
fn scope_from(raw: &str) -> Result<RoutingBindingScope> {
    match raw {
        "user" => Ok(RoutingBindingScope::User),
        "session" => Ok(RoutingBindingScope::Session),
        _ => Err(Error::Storage("invalid routing binding scope")),
    }
}
fn bounded_label(value: &str, max: usize, what: &'static str) -> Result<()> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-@".contains(&b))
    {
        return Err(Error::Config(what.into()));
    }
    Ok(())
}
fn digest(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Config(
            "routing digest must be canonical SHA-256 hex".into(),
        ));
    }
    Ok(())
}

fn strategy_row(
    c: &Connection,
    selector: &str,
    revision: Option<&str>,
) -> Result<RoutingStrategyRevisionInfo> {
    let row: Option<(String, String, i64, String, String, String, String, i64)> = if let Some(
        revision,
    ) = revision
    {
        db(c.query_row(
                "SELECT s.id,r.revision_id,r.revision_no,s.name,s.owner_user_id,r.definition_sha256,r.definition_json,r.created_at
                 FROM routing_strategies s JOIN routing_strategy_revisions r ON r.strategy_id=s.id
                 WHERE (s.id=?1 OR s.name=?1) AND (r.revision_id=?2 OR CAST(r.revision_no AS TEXT)=?2)
                   AND s.status='active'",
                params![selector, revision],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
            ).optional())?
    } else {
        db(c.query_row(
                "SELECT s.id,r.revision_id,r.revision_no,s.name,s.owner_user_id,r.definition_sha256,r.definition_json,r.created_at
                 FROM routing_strategies s JOIN routing_strategy_revisions r ON r.strategy_id=s.id
                 WHERE (s.id=?1 OR s.name=?1) AND s.status='active'
                 ORDER BY r.revision_no DESC LIMIT 1",
                [selector],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
            ).optional())?
    };
    let Some((
        strategy_id,
        revision_id,
        revision_no,
        name,
        owner_user_id,
        digest,
        body,
        created_at,
    )) = row
    else {
        return Err(Error::Denied(
            "routing strategy unavailable or not permitted",
        ));
    };
    let definition: RoutingStrategyDefinition = decode(&body)?;
    routing_error(validate_strategy(&definition))?;
    Ok(RoutingStrategyRevisionInfo {
        strategy_id,
        revision_id,
        revision_no: revision_no as u64,
        name,
        owner_user_id,
        definition_sha256: digest,
        definition,
        created_at,
    })
}

pub(crate) fn routing_target_destination_fingerprint(
    c: &Connection,
    revision_id: &str,
    target_id: &str,
) -> Result<String> {
    let strategy = strategy_revision_row(c, revision_id)?;
    let target = routing_error(strategy_target(&strategy.definition, target_id))?;
    Ok(target.destination_fingerprint.clone())
}

pub(crate) fn strategy_revision_row(
    c: &Connection,
    revision_id: &str,
) -> Result<RoutingStrategyRevisionInfo> {
    let row: Option<(String, String, i64, String, String, String, String, i64)> = db(c
        .query_row(
            "SELECT s.id,r.revision_id,r.revision_no,s.name,s.owner_user_id,r.definition_sha256,r.definition_json,r.created_at
             FROM routing_strategies s JOIN routing_strategy_revisions r ON r.strategy_id=s.id
             WHERE r.revision_id=?1 AND s.status='active'",
            [revision_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
        )
        .optional())?;
    let Some((
        strategy_id,
        revision_id,
        revision_no,
        name,
        owner_user_id,
        digest,
        body,
        created_at,
    )) = row
    else {
        return Err(Error::Denied("routing strategy revision unavailable"));
    };
    let definition: RoutingStrategyDefinition = decode(&body)?;
    routing_error(validate_strategy(&definition))?;
    Ok(RoutingStrategyRevisionInfo {
        strategy_id,
        revision_id,
        revision_no: revision_no as u64,
        name,
        owner_user_id,
        definition_sha256: digest,
        definition,
        created_at,
    })
}

fn strategy_row_owned(
    c: &Connection,
    owner_user_id: &str,
    selector: &str,
    revision: Option<&str>,
) -> Result<RoutingStrategyRevisionInfo> {
    let id: Option<String> = db(c
        .query_row(
            "SELECT id FROM routing_strategies WHERE owner_user_id=?1 AND (id=?2 OR name=?2) AND status='active'",
            params![owner_user_id, selector],
            |r| r.get(0),
        )
        .optional())?;
    strategy_row(
        c,
        &id.ok_or(Error::Denied(
            "routing strategy unavailable or not permitted",
        ))?,
        revision,
    )
}

fn binding_row(c: &Connection, id: &str) -> Result<RoutingBindingInfo> {
    let row: Option<(String, String, String, Option<String>, Option<String>, String, i64, String)> = db(c
        .query_row(
            "SELECT strategy_id,revision_id,scope_kind,scope_user_id,scope_session_id,role,revision,status FROM routing_bindings WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
        )
        .optional())?;
    let Some((strategy_id, revision_id, scope, user, session, role, revision, status)) = row else {
        return Err(Error::Denied("routing binding unavailable"));
    };
    Ok(RoutingBindingInfo {
        id: id.into(),
        strategy_id,
        revision_id,
        scope: scope_from(&scope)?,
        scope_user_id: user,
        scope_session_id: session,
        role,
        revision: revision as u64,
        status,
    })
}

fn operation_row(c: &Connection, id: &str) -> Result<RoutingOperationInfo> {
    let row: Option<(String,String,String,String,String,String,i64,String,String,String,String,String,String,Option<String>,Option<String>,Option<i64>,Option<String>,i64,i64)> = db(c
        .query_row(
            "SELECT strategy_id,revision_id,actor_user_id,session_id,branch_id,branch_head,branch_revision,role,lineage,mode,selection_source,input_sha256,candidate_ids_sha256,selected_target_id,working_identity_json,consumed_at,error,created_at,updated_at
             FROM routing_operations WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?,r.get(14)?,r.get(15)?,r.get(16)?,r.get(17)?,r.get(18)?)),
        )
        .optional())?;
    let Some((
        strategy_id,
        revision_id,
        actor_user_id,
        session_id,
        branch_id,
        branch_head,
        branch_revision,
        role,
        lineage,
        mode,
        selection_source,
        input_sha256,
        candidate_ids_sha256,
        selected_target_id,
        working_identity,
        consumed_at,
        error,
        created_at,
        updated_at,
    )) = row
    else {
        return Err(Error::Denied("routing operation unavailable"));
    };
    let state: String = db(c.query_row(
        "SELECT state FROM routing_operations WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    Ok(RoutingOperationInfo {
        id: id.into(),
        strategy_id,
        revision_id,
        actor_user_id,
        session_id,
        branch_id,
        branch_head,
        branch_revision: branch_revision as u64,
        role,
        lineage,
        mode: mode_from(&mode)?,
        selection_source: source_from(&selection_source)?,
        input_sha256,
        candidate_ids_sha256,
        state,
        selected_target_id,
        working_identity: working_identity.map(|value| decode(&value)).transpose()?,
        consumed_at,
        error,
        created_at,
        updated_at,
    })
}

fn attempt_row(c: &Connection, operation_id: &str) -> Result<Option<RoutingAttemptInfo>> {
    let row: Option<(String,i64,String,String,Option<String>,Option<String>,Option<String>,String,i64,i64,Option<i64>)> = db(c
        .query_row(
            "SELECT id,attempt_no,classifier_identity_json,state,metadata_json,selected_target_id,error,owner_process_id,owner_store_generation,started_at,settled_at
             FROM routing_attempts WHERE operation_id=?1",
            [operation_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?)),
        )
        .optional())?;
    row.map(
        |(
            id,
            attempt_no,
            identity,
            state,
            metadata,
            target,
            error,
            process,
            generation,
            started,
            settled,
        )| {
            Ok(RoutingAttemptInfo {
                id,
                operation_id: operation_id.into(),
                attempt_no: attempt_no as u32,
                classifier_identity: decode(&identity)?,
                state,
                metadata: metadata.map(|v| decode(&v)).transpose()?,
                selected_target_id: target,
                error,
                owner_process_id: process,
                owner_store_generation: generation as u64,
                started_at: started,
                settled_at: settled,
            })
        },
    )
    .transpose()
}

fn affinity_row(
    c: &Connection,
    actor_user: &str,
    session: &str,
    branch: &str,
    role: &str,
    lineage: &str,
) -> Result<Option<RoutingAffinityInfo>> {
    let row: Option<(String, String, String, i64, i64, i64)> = db(c
        .query_row(
            "SELECT strategy_revision_id,target_id,established_operation_id,revision,created_at,updated_at
             FROM routing_affinities WHERE actor_user_id=?1 AND session_id=?2 AND branch_id=?3 AND role=?4 AND lineage=?5",
            params![actor_user, session, branch, role, lineage],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional())?;
    Ok(row.map(
        |(strategy_revision_id, target_id, operation, revision, created_at, updated_at)| {
            RoutingAffinityInfo {
                actor_user_id: actor_user.into(),
                session_id: session.into(),
                branch_id: branch.into(),
                role: role.into(),
                lineage: lineage.into(),
                strategy_revision_id,
                target_id,
                established_operation_id: operation,
                revision: revision as u64,
                created_at,
                updated_at,
            }
        },
    ))
}

fn authorize_route_scope(
    c: &Connection,
    actor: &PrincipalId,
    session: &str,
    branch: &str,
) -> Result<(String, BranchInfo)> {
    authorize(c, session, actor, true)?;
    let user = ensure_user_for_principal(c, actor)?;
    let branch = branch_in(c, session, branch)?;
    Ok((user, branch))
}

pub(crate) fn consume_working_route_in(
    c: &Connection,
    actor: &PrincipalId,
    operation_id: &str,
    session: &str,
    branch: &BranchInfo,
    identity: &InvocationIdentity,
) -> Result<()> {
    let user = ensure_user_for_principal(c, actor)?;
    let op = operation_row(c, operation_id)?;
    if op.actor_user_id != user
        || op.session_id != session
        || op.branch_id != branch.id
        || op.branch_head != branch.head
        || op.branch_revision != branch.revision
        || op.state != "selected"
        || op.consumed_at.is_some()
        || identity.actor != *actor
    {
        return Err(Error::Conflict(
            "routing receipt does not match this exact working Chat admission",
        ));
    }
    let target_id = op
        .selected_target_id
        .as_deref()
        .ok_or(Error::Storage("selected routing receipt has no target"))?;
    let strategy = strategy_revision_row(c, &op.revision_id)?;
    let target = routing_error(strategy_target(&strategy.definition, target_id))?;
    if target.account_id != identity.account_id || target.model != identity.model {
        return Err(Error::Denied(
            "working provider selection differs from the durable routing receipt",
        ));
    }
    let changed = db(c.execute(
        "UPDATE routing_operations
         SET working_identity_json=?1,consumed_at=?2,updated_at=?2
         WHERE id=?3 AND state='selected' AND consumed_at IS NULL",
        params![json(identity)?, now(), operation_id],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("routing receipt was already consumed"));
    }
    Ok(())
}

impl SessionStore {
    pub fn routing_strategy_define(
        &self,
        actor: &PrincipalId,
        definition: RoutingStrategyDefinition,
    ) -> Result<RoutingStrategyRevisionInfo> {
        routing_error(validate_strategy(&definition))?;
        if definition
            .candidates
            .iter()
            .chain(definition.classifier.iter())
            .any(|target| target.destination_fingerprint.len() != 64)
        {
            return Err(Error::Config(
                "routing strategies must pin each configured destination before persistence".into(),
            ));
        }
        let digest = routing_error(strategy_sha256(&definition))?;
        let body = json(&definition)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let existing: Option<String> = db(tx
            .query_row(
                "SELECT id FROM routing_strategies WHERE owner_user_id=?1 AND name=?2 AND status='active'",
                params![user, definition.name],
                |r| r.get(0),
            )
            .optional())?;
        let strategy_id = if let Some(id) = existing {
            id
        } else {
            let count: i64 =
                db(tx.query_row("SELECT count(*) FROM routing_strategies", [], |r| r.get(0)))?;
            if count >= MAX_ROUTING_STRATEGIES {
                return Err(Error::Limit("routing strategy limit reached"));
            }
            let id = id("route")?;
            db(tx.execute(
                "INSERT INTO routing_strategies(id,owner_user_id,name,status,created_at) VALUES(?1,?2,?3,'active',?4)",
                params![id,user,definition.name,now()],
            ))?;
            id
        };
        if let Some(revision_id) = db(tx
            .query_row(
                "SELECT revision_id FROM routing_strategy_revisions WHERE strategy_id=?1 AND definition_sha256=?2",
                params![strategy_id,digest],
                |r| r.get::<_,String>(0),
            )
            .optional())?
        {
            let out = strategy_row(&tx, &strategy_id, Some(&revision_id))?;
            db(tx.commit())?;
            return Ok(out);
        }
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM routing_strategy_revisions WHERE strategy_id=?1",
            [&strategy_id],
            |r| r.get(0),
        ))?;
        if count >= MAX_ROUTING_REVISIONS {
            return Err(Error::Limit("routing strategy revision limit reached"));
        }
        let revision_no: i64 = db(tx.query_row(
            "SELECT coalesce(max(revision_no),0)+1 FROM routing_strategy_revisions WHERE strategy_id=?1",
            [&strategy_id],
            |r| r.get(0),
        ))?;
        let revision_id = id("rrev")?;
        db(tx.execute(
            "INSERT INTO routing_strategy_revisions(revision_id,strategy_id,revision_no,definition_sha256,definition_json,created_by_user_id,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![revision_id,strategy_id,revision_no,digest,body,user,now()],
        ))?;
        let out = strategy_row(&tx, &strategy_id, Some(&revision_id))?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn routing_strategy(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
    ) -> Result<RoutingStrategyRevisionInfo> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        strategy_row_owned(&c, &user, selector, revision)
    }

    pub fn routing_strategies(
        &self,
        actor: &PrincipalId,
    ) -> Result<Vec<RoutingStrategyRevisionInfo>> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        let mut q = db(c.prepare(
            "SELECT id FROM routing_strategies WHERE owner_user_id=?1 AND status='active' ORDER BY name,id LIMIT 1024",
        ))?;
        let ids = db(db(q.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| strategy_row(&c, id, None)).collect()
    }

    pub fn routing_bind(
        &self,
        actor: &PrincipalId,
        selector: &str,
        revision: Option<&str>,
        scope: RoutingBindingScope,
        scope_id: &str,
        role: &str,
    ) -> Result<RoutingBindingInfo> {
        bounded_label(role, 64, "invalid routing role")?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let strategy = strategy_row_owned(&tx, &user, selector, revision)?;
        if strategy.definition.role != role {
            return Err(Error::Conflict(
                "routing binding role differs from strategy role",
            ));
        }
        let (scope_user, scope_session) = match scope {
            RoutingBindingScope::User => {
                let target = if scope_id == "self" {
                    user.clone()
                } else {
                    scope_id.into()
                };
                if target != user {
                    return Err(Error::Denied(
                        "Phase 40 user routing bindings are owner-self only",
                    ));
                }
                (Some(target), None)
            }
            RoutingBindingScope::Session => {
                let session = session_in(&tx, scope_id)?;
                if !principal_is_session_owner(&tx, &session.id, actor)? {
                    return Err(Error::Denied(
                        "Session owner required to bind routing strategy",
                    ));
                }
                (None, Some(session.id))
            }
        };
        let active: i64 = db(tx.query_row(
            "SELECT count(*) FROM routing_bindings WHERE role=?1 AND status='active' AND ((scope_kind='user' AND scope_user_id=?2) OR (scope_kind='session' AND scope_session_id=?3))",
            params![role,scope_user,scope_session],
            |r| r.get(0),
        ))?;
        if active != 0 {
            return Err(Error::Conflict(
                "an active routing binding already owns this scope/role",
            ));
        }
        let id = id("rbind")?;
        let at = now();
        db(tx.execute(
            "INSERT INTO routing_bindings(id,strategy_id,revision_id,scope_kind,scope_user_id,scope_session_id,role,revision,status,created_by_user_id,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,1,'active',?8,?9,?9)",
            params![id,strategy.strategy_id,strategy.revision_id,scope.as_str(),scope_user,scope_session,role,user,at],
        ))?;
        let out = binding_row(&tx, &id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn routing_bindings(&self, actor: &PrincipalId) -> Result<Vec<RoutingBindingInfo>> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        let mut q = db(c.prepare(
            "SELECT id FROM routing_bindings WHERE created_by_user_id=?1 ORDER BY created_at,id LIMIT 4096",
        ))?;
        let ids = db(db(q.query_map([user], |r| r.get::<_, String>(0)))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
        ids.iter().map(|id| binding_row(&c, id)).collect()
    }

    pub fn routing_binding_set(
        &self,
        actor: &PrincipalId,
        binding_id: &str,
        expected_revision: u64,
        enabled: bool,
        revision_id: Option<&str>,
    ) -> Result<RoutingBindingInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let current = binding_row(&tx, binding_id)?;
        let creator: String = db(tx.query_row(
            "SELECT created_by_user_id FROM routing_bindings WHERE id=?1",
            [binding_id],
            |r| r.get(0),
        ))?;
        if creator != user || current.revision != expected_revision {
            return Err(Error::Conflict("routing binding revision/owner changed"));
        }
        let next_revision = if let Some(revision_id) = revision_id {
            let strategy = strategy_row_owned(&tx, &user, &current.strategy_id, Some(revision_id))?;
            if strategy.definition.role != current.role {
                return Err(Error::Conflict(
                    "replacement routing revision has another role",
                ));
            }
            strategy.revision_id
        } else {
            current.revision_id.clone()
        };
        let changed = db(tx.execute(
            "UPDATE routing_bindings SET revision_id=?1,status=?2,revision=revision+1,updated_at=?3
             WHERE id=?4 AND revision=?5 AND created_by_user_id=?6",
            params![
                next_revision,
                if enabled { "active" } else { "disabled" },
                now(),
                binding_id,
                expected_revision,
                user
            ],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("routing binding CAS lost"));
        }
        let out = binding_row(&tx, binding_id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn routing_resolve(
        &self,
        actor: &PrincipalId,
        explicit_selector: Option<&str>,
        explicit_revision: Option<&str>,
        session: &str,
        branch: &str,
        role: &str,
    ) -> Result<Option<RoutingResolution>> {
        bounded_label(role, 64, "invalid routing role")?;
        let c = self.connection()?;
        let (user, _) = authorize_route_scope(&c, actor, session, branch)?;
        if let Some(selector) = explicit_selector {
            let strategy = strategy_row_owned(&c, &user, selector, explicit_revision)?;
            if strategy.definition.role != role {
                return Err(Error::Conflict(
                    "selected routing strategy has another role",
                ));
            }
            return Ok(Some(RoutingResolution {
                strategy,
                binding: None,
            }));
        }
        if explicit_revision.is_some() {
            return Err(Error::Config(
                "--route-revision requires an explicit route strategy".into(),
            ));
        }
        let binding_id: Option<String> = db(c
            .query_row(
                "SELECT id FROM routing_bindings WHERE scope_kind='session' AND scope_session_id=?1 AND role=?2 AND status='active' ORDER BY updated_at DESC,id LIMIT 1",
                params![session,role],
                |r| r.get(0),
            )
            .optional())?
            .or(db(c
                .query_row(
                    "SELECT id FROM routing_bindings WHERE scope_kind='user' AND scope_user_id=?1 AND role=?2 AND status='active' ORDER BY updated_at DESC,id LIMIT 1",
                    params![user,role],
                    |r| r.get(0),
                )
                .optional())?);
        let Some(binding_id) = binding_id else {
            return Ok(None);
        };
        let binding = binding_row(&c, &binding_id)?;
        let strategy = strategy_row(&c, &binding.strategy_id, Some(&binding.revision_id))?;
        if strategy.definition.role != role {
            return Err(Error::Storage("routing binding/strategy role mismatch"));
        }
        Ok(Some(RoutingResolution {
            strategy,
            binding: Some(binding),
        }))
    }

    pub fn routing_affinity(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        role: &str,
        lineage: &str,
    ) -> Result<Option<RoutingAffinityInfo>> {
        bounded_label(role, 64, "invalid routing role")?;
        bounded_label(lineage, 128, "invalid routing lineage")?;
        let c = self.connection()?;
        let (user, branch) = authorize_route_scope(&c, actor, session, branch)?;
        affinity_row(&c, &user, session, &branch.id, role, lineage)
    }

    pub fn routing_affinity_clear(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        role: &str,
        lineage: &str,
        expected_revision: u64,
    ) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (user, branch) = authorize_route_scope(&tx, actor, session, branch)?;
        let changed = db(tx.execute(
            "DELETE FROM routing_affinities WHERE actor_user_id=?1 AND session_id=?2 AND branch_id=?3 AND role=?4 AND lineage=?5 AND revision=?6",
            params![user,session,branch.id,role,lineage,expected_revision],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("routing affinity CAS lost"));
        }
        db(tx.commit())?;
        Ok(())
    }

    fn routing_admit(
        &self,
        actor: &PrincipalId,
        strategy_revision_id: &str,
        session: &str,
        branch: &str,
        role: &str,
        lineage: &str,
        input_sha256: &str,
        candidate_ids_sha256: &str,
        source: RoutingSelectionSource,
        target_id: Option<&str>,
    ) -> Result<RoutingOperationInfo> {
        bounded_label(role, 64, "invalid routing role")?;
        bounded_label(lineage, 128, "invalid routing lineage")?;
        digest(input_sha256)?;
        digest(candidate_ids_sha256)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (user, branch) = authorize_route_scope(&tx, actor, session, branch)?;
        let strategy = strategy_revision_row(&tx, strategy_revision_id)?;
        if strategy.definition.role != role {
            return Err(Error::Conflict(
                "routing operation role differs from strategy role",
            ));
        }
        match source {
            RoutingSelectionSource::Fixed => {
                if strategy.definition.mode != RoutingMode::Fixed
                    || target_id != strategy.definition.fixed_target.as_deref()
                {
                    return Err(Error::Conflict(
                        "fixed routing selection differs from strategy",
                    ));
                }
            }
            RoutingSelectionSource::Affinity => {
                if strategy.definition.mode != RoutingMode::RouteOnce {
                    return Err(Error::Conflict(
                        "affinity is valid only for route-once strategy",
                    ));
                }
                let affinity = affinity_row(&tx, &user, session, &branch.id, role, lineage)?
                    .ok_or(Error::Conflict(
                        "routing affinity disappeared; reselect explicitly",
                    ))?;
                if affinity.strategy_revision_id != strategy.revision_id
                    || Some(affinity.target_id.as_str()) != target_id
                {
                    return Err(Error::Conflict(
                        "routing affinity differs from selected strategy/target",
                    ));
                }
            }
            RoutingSelectionSource::Classifier => {
                if !matches!(
                    strategy.definition.mode,
                    RoutingMode::RouteOnce | RoutingMode::RouteEachPrompt
                ) || target_id.is_some()
                {
                    return Err(Error::Conflict(
                        "classifier admission differs from routing strategy",
                    ));
                }
            }
        }
        if let Some(target_id) = target_id {
            let _ = routing_error(strategy_target(&strategy.definition, target_id))?;
        }
        let count: i64 =
            db(tx.query_row("SELECT count(*) FROM routing_operations", [], |r| r.get(0)))?;
        if count >= MAX_ROUTING_OPERATIONS {
            return Err(Error::Limit(
                "routing operation limit reached; rotate/archive store explicitly",
            ));
        }
        let id = id("routeop")?;
        let state = if target_id.is_some() {
            "selected"
        } else {
            "admitted"
        };
        let at = now();
        db(tx.execute(
            "INSERT INTO routing_operations(id,strategy_id,revision_id,actor_user_id,session_id,branch_id,branch_head,branch_revision,role,lineage,mode,selection_source,input_sha256,candidate_ids_sha256,state,selected_target_id,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17)",
            params![id,strategy.strategy_id,strategy.revision_id,user,session,branch.id,branch.head,branch.revision,role,lineage,mode_str(strategy.definition.mode),source.as_str(),input_sha256,candidate_ids_sha256,state,target_id,at],
        ))?;
        let out = operation_row(&tx, &id)?;
        db(tx.commit())?;
        Ok(out)
    }

    pub fn routing_admit_resolved(
        &self,
        actor: &PrincipalId,
        strategy_revision_id: &str,
        session: &str,
        branch: &str,
        role: &str,
        lineage: &str,
        input_sha256: &str,
        candidate_ids_sha256: &str,
        source: RoutingSelectionSource,
        target_id: &str,
    ) -> Result<RoutingOperationInfo> {
        if source == RoutingSelectionSource::Classifier {
            return Err(Error::Config(
                "resolved routing source cannot be classifier".into(),
            ));
        }
        self.routing_admit(
            actor,
            strategy_revision_id,
            session,
            branch,
            role,
            lineage,
            input_sha256,
            candidate_ids_sha256,
            source,
            Some(target_id),
        )
    }

    pub fn routing_admit_classifier(
        &self,
        actor: &PrincipalId,
        strategy_revision_id: &str,
        session: &str,
        branch: &str,
        role: &str,
        lineage: &str,
        input_sha256: &str,
        candidate_ids_sha256: &str,
    ) -> Result<RoutingOperationInfo> {
        self.routing_admit(
            actor,
            strategy_revision_id,
            session,
            branch,
            role,
            lineage,
            input_sha256,
            candidate_ids_sha256,
            RoutingSelectionSource::Classifier,
            None,
        )
    }

    /// Settle only a classifier operation that never crossed the provider-intent
    /// boundary. Once an attempt exists/started, recovery/settlement owns it and
    /// this method deliberately does nothing.
    pub fn routing_settle_unstarted(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        cancelled: bool,
        reason: &str,
    ) -> Result<bool> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let op = operation_row(&tx, operation_id)?;
        if op.actor_user_id != user || op.selection_source != RoutingSelectionSource::Classifier {
            return Err(Error::Denied("routing operation unavailable"));
        }
        // Recording the terminal pre-dispatch state is allowed after a
        // permission revocation: it grants no new effect and prevents stale
        // admitted work from looking runnable. Actor ownership was checked above.
        let mut reason = reason.replace('\0', "");
        reason.truncate(1024);
        if reason.is_empty() {
            reason = if cancelled {
                "routing cancelled before classifier dispatch"
            } else {
                "routing failed before classifier dispatch"
            }
            .into();
        }
        let changed = db(tx.execute(
            "UPDATE routing_operations SET state=?1,error=?2,updated_at=?3
             WHERE id=?4 AND state='admitted'
               AND NOT EXISTS(SELECT 1 FROM routing_attempts a WHERE a.operation_id=routing_operations.id)",
            params![if cancelled { "cancelled" } else { "failed" },reason,now(),operation_id],
        ))?;
        db(tx.commit())?;
        Ok(changed == 1)
    }

    pub fn routing_consume_for_delegation(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        allocation_id: &str,
        identity: &InvocationIdentity,
    ) -> Result<crate::subagents::DelegationChildAllocationInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let op = operation_row(&tx, operation_id)?;
        let allocation = crate::subagents::delegation_allocation_row(&tx, allocation_id)?;
        let root = crate::subagents::delegation_root_row(&tx, &allocation.root_id)?;
        if op.actor_user_id != user
            || identity.actor != *actor
            || !principal_is_session_owner(&tx, &root.root_session_id, actor)?
            || allocation.state != "reserved"
            || allocation.routing_operation_id.is_some()
            || allocation.selected_identity.is_some()
            || op.state != "selected"
            || op.consumed_at.is_some()
            || op.session_id != allocation.parent_session_id
            || op.branch_id != allocation.parent_branch_id
            || op.role != "worker"
            || root.child_routing_revision_id.as_deref() != Some(op.revision_id.as_str())
        {
            return Err(Error::Denied(
                "selected routing operation cannot be consumed by this child allocation",
            ));
        }
        let target_id = op.selected_target_id.as_deref().ok_or(Error::Storage(
            "selected child routing operation has no target",
        ))?;
        let strategy = strategy_revision_row(&tx, &op.revision_id)?;
        let target = routing_error(strategy_target(&strategy.definition, target_id))?;
        if target.account_id != identity.account_id || target.model != identity.model {
            return Err(Error::Denied(
                "child routing working identity differs from the selected target",
            ));
        }
        let at = now();
        let operation_changed = db(tx.execute(
            "UPDATE routing_operations
             SET working_identity_json=?1,consumed_at=?2,updated_at=?2
             WHERE id=?3 AND state='selected' AND consumed_at IS NULL",
            params![json(identity)?, at, operation_id],
        ))?;
        if operation_changed != 1 {
            return Err(Error::Conflict(
                "child routing operation was consumed concurrently",
            ));
        }
        let allocation_changed = db(tx.execute(
            "UPDATE delegation_child_allocations
             SET routing_operation_id=?1,routing_target_id=?2,routing_selection_source=?3,
                 selected_identity_json=?4,routing_bound_at=?5
             WHERE id=?6 AND state='reserved' AND routing_operation_id IS NULL
               AND selected_identity_json IS NULL",
            params![
                operation_id,
                target_id,
                op.selection_source.as_str(),
                json(identity)?,
                at,
                allocation_id
            ],
        ))?;
        if allocation_changed != 1 {
            return Err(Error::Conflict(
                "child allocation routing selection was consumed concurrently",
            ));
        }
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "delegation.route_consumed",
            "delegation_child_allocation",
            allocation_id,
            Some(actor),
            serde_json::json!({
                "routing_operation_id": operation_id,
                "routing_target_id": target_id,
                "selection_source": op.selection_source.as_str(),
                "account_id": identity.account_id,
                "model": identity.model,
            }),
        )?;
        db(tx.commit())?;
        crate::subagents::delegation_allocation_row(&c, allocation_id)
    }

    pub fn routing_operation(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
    ) -> Result<RoutingOperationInfo> {
        let c = self.connection()?;
        let user = ensure_user_for_principal(&c, actor)?;
        let op = operation_row(&c, operation_id)?;
        if op.actor_user_id != user {
            return Err(Error::Denied("routing operation unavailable"));
        }
        authorize(&c, &op.session_id, actor, false)?;
        Ok(op)
    }

    pub fn routing_attempt(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
    ) -> Result<Option<RoutingAttemptInfo>> {
        let _ = self.routing_operation(actor, operation_id)?;
        let c = self.connection()?;
        attempt_row(&c, operation_id)
    }

    pub fn routing_classifier_journal(
        &self,
        actor: PrincipalId,
        operation_id: &str,
    ) -> Result<RoutingJournalHandle> {
        let op = self.routing_operation(&actor, operation_id)?;
        if op.selection_source != RoutingSelectionSource::Classifier || op.state != "admitted" {
            return Err(Error::Conflict(
                "routing operation is not awaiting a classifier attempt",
            ));
        }
        Ok(RoutingJournalHandle {
            store: self.clone(),
            actor,
            operation_id: operation_id.into(),
        })
    }

    pub fn routing_affinity_establish(
        &self,
        actor: &PrincipalId,
        operation_id: &str,
        expected_revision: Option<u64>,
    ) -> Result<RoutingAffinityInfo> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let op = operation_row(&tx, operation_id)?;
        if op.actor_user_id != user || op.mode != RoutingMode::RouteOnce || op.state != "selected" {
            return Err(Error::Conflict(
                "routing operation cannot establish route-once affinity",
            ));
        }
        let branch = branch_in(&tx, &op.session_id, &op.branch_id)?;
        authorize(&tx, &op.session_id, actor, true)?;
        if branch.head != op.branch_head || branch.revision != op.branch_revision {
            db(tx.execute(
                "UPDATE routing_operations SET state='conflict',error='branch moved before route-once affinity CAS',updated_at=?1 WHERE id=?2 AND state='selected'",
                params![now(),operation_id],
            ))?;
            db(tx.commit())?;
            return Err(Error::Conflict(
                "branch moved before routing affinity could attach",
            ));
        }
        let target = op
            .selected_target_id
            .as_deref()
            .ok_or(Error::Storage("selected routing operation has no target"))?;
        let strategy = strategy_revision_row(&tx, &op.revision_id)?;
        let _ = routing_error(strategy_target(&strategy.definition, target))?;
        let current = affinity_row(
            &tx,
            &user,
            &op.session_id,
            &op.branch_id,
            &op.role,
            &op.lineage,
        )?;
        let at = now();
        match (current, expected_revision) {
            (None, None) => {
                db(tx.execute(
                    "INSERT INTO routing_affinities(actor_user_id,session_id,branch_id,role,lineage,strategy_revision_id,target_id,established_operation_id,revision,created_at,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?9)",
                    params![user,op.session_id,op.branch_id,op.role,op.lineage,op.revision_id,target,operation_id,at],
                ))?;
            }
            (Some(current), Some(expected)) if current.revision == expected => {
                let changed = db(tx.execute(
                    "UPDATE routing_affinities SET strategy_revision_id=?1,target_id=?2,established_operation_id=?3,revision=revision+1,updated_at=?4
                     WHERE actor_user_id=?5 AND session_id=?6 AND branch_id=?7 AND role=?8 AND lineage=?9 AND revision=?10",
                    params![op.revision_id,target,operation_id,at,user,op.session_id,op.branch_id,op.role,op.lineage,expected],
                ))?;
                if changed != 1 {
                    return Err(Error::Conflict("routing affinity CAS lost"));
                }
            }
            _ => {
                db(tx.execute(
                    "UPDATE routing_operations SET state='conflict',error='route-once affinity CAS lost; paid classifier result preserved',updated_at=?1 WHERE id=?2 AND state='selected'",
                    params![at,operation_id],
                ))?;
                db(tx.commit())?;
                return Err(Error::Conflict(
                    "routing affinity changed concurrently; paid selection is preserved but not attached",
                ));
            }
        }
        let out = affinity_row(
            &tx,
            &user,
            &op.session_id,
            &op.branch_id,
            &op.role,
            &op.lineage,
        )?
        .ok_or(Error::Storage("routing affinity commit disappeared"))?;
        db(tx.commit())?;
        Ok(out)
    }
}

#[derive(Clone)]
pub struct RoutingJournalHandle {
    store: SessionStore,
    actor: PrincipalId,
    operation_id: String,
}

impl RoutingJournalHandle {
    fn authorize_in(
        &self,
        c: &Connection,
        input: &RoutingInput,
        identity: &InvocationIdentity,
        states: &[&str],
    ) -> Result<RoutingOperationInfo> {
        let user = ensure_user_for_principal(c, &self.actor)?;
        let op = operation_row(c, &self.operation_id)?;
        if op.actor_user_id != user
            || !states.contains(&op.state.as_str())
            || op.revision_id != input.evidence.strategy_revision_id
            || op.role != input.evidence.role
            || op.lineage != input.evidence.lineage
            || op.input_sha256 != input.digest()?
            || op.candidate_ids_sha256 != input.candidate_ids_digest()?
        {
            return Err(Error::Denied("routing operation/input authority mismatch"));
        }
        authorize(c, &op.session_id, &self.actor, true)?;
        let branch = branch_in(c, &op.session_id, &op.branch_id)?;
        if branch.head != op.branch_head || branch.revision != op.branch_revision {
            return Err(Error::Conflict(
                "routing source branch changed; working prompt must be re-admitted",
            ));
        }
        let strategy = strategy_revision_row(c, &op.revision_id)?;
        let classifier = strategy
            .definition
            .classifier
            .as_ref()
            .ok_or(Error::Storage(
                "classified routing strategy lost classifier",
            ))?;
        if classifier.account_id != identity.account_id || classifier.model != identity.model {
            return Err(Error::Denied(
                "routing classifier identity differs from strategy",
            ));
        }
        if op.state == "classifier_started" {
            let attempt = attempt_row(c, &op.id)?.ok_or(Error::Storage(
                "classifier-started operation has no attempt",
            ))?;
            if attempt.classifier_identity != *identity {
                return Err(Error::Denied("routing attempt identity changed"));
            }
        }
        Ok(op)
    }
}

impl RoutingJournal for RoutingJournalHandle {
    fn authorize<'a>(
        &'a self,
        input: &'a RoutingInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let c = self.store.connection()?;
            self.authorize_in(&c, input, identity, &["admitted", "classifier_started"])?;
            Ok(())
        })
    }

    fn begin<'a>(
        &'a self,
        input: &'a RoutingInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            let process = self.store.process_identity()?.ok_or(Error::Denied(
                "paid routing classifier requires durable process ownership",
            ))?;
            let mut c = self.store.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let op = self.authorize_in(&tx, input, identity, &["admitted"])?;
            let attempt = id("routeatt")?;
            let scope = format!("routing:{}", op.id);
            coordination::acquire_execution_lease_in(
                &tx,
                &process,
                &scope,
                &attempt,
                Duration::from_secs(30 * 60),
            )?;
            db(tx.execute(
                "INSERT INTO routing_attempts(id,operation_id,attempt_no,classifier_identity_json,state,owner_process_id,owner_store_generation,started_at)
                 VALUES(?1,?2,1,?3,'started',?4,?5,?6)",
                params![attempt,op.id,json(identity)?,process.process_id,process.store_generation,now()],
            ))?;
            let changed = db(tx.execute(
                "UPDATE routing_operations SET state='classifier_started',updated_at=?1 WHERE id=?2 AND state='admitted'",
                params![now(),op.id],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict("routing classifier admission CAS lost"));
            }
            db(tx.commit())?;
            Ok(InvocationContext {
                purpose: InvocationPurpose::Routing {
                    operation_id: op.id,
                    input_sha256: input.digest()?,
                    strategy_revision_id: input.evidence.strategy_revision_id.clone(),
                    candidate_ids_sha256: input.candidate_ids_digest()?,
                },
                scope: Some(aw_agent_core::InvocationScope {
                    session_id: op.session_id,
                    branch_id: op.branch_id,
                }),
                turn_id: 0,
                ordinal: 1,
                identity: identity.clone(),
            })
        })
    }

    fn finish<'a>(&'a self, result: &'a RoutingResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let process = self.store.process_identity()?.ok_or(Error::Denied(
                "routing settlement requires durable process ownership",
            ))?;
            let mut c = self.store.connection()?;
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let op = operation_row(&tx, &self.operation_id)?;
            let user = ensure_user_for_principal(&tx, &self.actor)?;
            if op.actor_user_id != user || op.state != "classifier_started" {
                return Err(Error::Conflict(
                    "routing operation is not awaiting classifier settlement",
                ));
            }
            let attempt = attempt_row(&tx, &op.id)?.ok_or(Error::Storage(
                "routing attempt disappeared before settlement",
            ))?;
            if attempt.state != "started"
                || attempt.owner_process_id != process.process_id
                || attempt.owner_store_generation != process.store_generation
            {
                return Err(Error::Conflict("routing attempt process ownership changed"));
            }
            let (attempt_state, operation_state, target) = match result.outcome {
                Outcome::Completed => {
                    let target = result
                        .proposal
                        .as_ref()
                        .ok_or(Error::Protocol("completed routing result has no proposal"))?
                        .candidate_id
                        .clone();
                    let strategy = strategy_revision_row(&tx, &op.revision_id)?;
                    let _ = routing_error(strategy_target(&strategy.definition, &target))?;
                    ("completed", "selected", Some(target))
                }
                Outcome::Cancelled => ("cancelled", "cancelled", None),
                Outcome::Failed | Outcome::Incomplete => ("failed", "failed", None),
            };
            let at = now();
            db(tx.execute(
                "UPDATE routing_attempts SET state=?1,metadata_json=?2,selected_target_id=?3,error=?4,settled_at=?5
                 WHERE id=?6 AND state='started'",
                params![attempt_state,result.meta.as_ref().map(json).transpose()?,target,result.error,at,attempt.id],
            ))?;
            db(tx.execute(
                "UPDATE routing_operations SET state=?1,selected_target_id=?2,error=?3,updated_at=?4
                 WHERE id=?5 AND state='classifier_started'",
                params![operation_state,target,result.error,at,op.id],
            ))?;
            coordination::settle_execution_scope_in(
                &tx,
                &process,
                &format!("routing:{}", op.id),
                &attempt.id,
            )?;
            db(tx.commit())?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AccountOwner, BillingOwner, CredentialCustody, InvocationIdentity, PrincipalId,
        TurnJournal, TurnStart,
    };
    use aw_routing::{RoutingStrategyDefinition, RoutingTargetRef};
    use std::path::PathBuf;

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/fixture"),
            tool_root: PathBuf::from("/fixture"),
            system_prompt: String::new(),
            profile_hint: "main".into(),
            model_hint: "work".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }
    fn target(id: &str, account: &str, model: &str) -> RoutingTargetRef {
        RoutingTargetRef {
            id: id.into(),
            account_id: account.into(),
            model: model.into(),
            destination_fingerprint: "a".repeat(64),
            description: id.into(),
        }
    }
    fn worker_strategy(mode: RoutingMode) -> RoutingStrategyDefinition {
        let mut value = strategy(mode);
        value.name = "worker-route".into();
        value.role = "worker".into();
        value
    }

    fn strategy(mode: RoutingMode) -> RoutingStrategyDefinition {
        let mut value = RoutingStrategyDefinition {
            name: "main-route".into(),
            role: "main".into(),
            mode,
            fixed_target: None,
            classifier: Some(target("judge", "router", "judge-model")),
            candidates: vec![
                target("fast", "fast", "fast-model"),
                target("deep", "deep", "deep-model"),
            ],
        };
        if mode == RoutingMode::Fixed {
            value.classifier = None;
            value.fixed_target = Some("fast".into());
        }
        value
    }
    fn sha(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }

    fn identity(actor: &PrincipalId, account: &str, model: &str) -> InvocationIdentity {
        InvocationIdentity {
            actor: actor.clone(),
            account_id: account.into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("local".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: model.into(),
            grant_revision: 1,
        }
    }

    #[test]
    fn delegation_root_accepts_only_worker_route_and_pins_revision() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let root_identity = identity(&actor, "main", "work");
        let (session, branch) = store.create(&actor, "parent", binding()).unwrap();
        let (_, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(handle.begin(TurnStart {
                turn_id: 1,
                prompt: "delegate".into(),
                identity: root_identity,
            }))
            .unwrap();
        let worker = store
            .routing_strategy_define(&actor, worker_strategy(RoutingMode::Fixed))
            .unwrap();
        let main = store
            .routing_strategy_define(&actor, strategy(RoutingMode::Fixed))
            .unwrap();
        let limits = DelegationLimits {
            max_children_created: 4,
            max_active_children: 2,
            max_model_attempts: 8,
            max_depth: 2,
            deadline_seconds: 3600,
        };
        assert!(store
            .reserve_delegation_child(
                &actor,
                &session.id,
                &branch.id,
                1,
                limits,
                Some(&main.revision_id),
                1,
            )
            .is_err());
        let (root, _) = store
            .reserve_delegation_child(
                &actor,
                &session.id,
                &branch.id,
                1,
                limits,
                Some(&worker.revision_id),
                1,
            )
            .unwrap();
        assert_eq!(root.children_created, 1);
        assert_eq!(
            root.child_routing_revision_id.as_deref(),
            Some(worker.revision_id.as_str())
        );
        assert!(store
            .reserve_delegation_child(&actor, &session.id, &branch.id, 1, limits, None, 1,)
            .is_err());
        assert_eq!(
            store
                .delegation_root(&actor, &root.id)
                .unwrap()
                .children_created,
            1,
        );
    }

    #[test]
    fn fixed_worker_route_consumes_once_into_child_allocation_and_selected_identity() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let root_identity = identity(&actor, "main", "work");
        let (session, branch) = store.create(&actor, "parent", binding()).unwrap();
        let (_, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(handle.begin(TurnStart {
                turn_id: 1,
                prompt: "delegate".into(),
                identity: root_identity,
            }))
            .unwrap();
        let strategy = store
            .routing_strategy_define(&actor, worker_strategy(RoutingMode::Fixed))
            .unwrap();
        let limits = DelegationLimits {
            max_children_created: 4,
            max_active_children: 2,
            max_model_attempts: 8,
            max_depth: 2,
            deadline_seconds: 3600,
        };
        let (root, allocation) = store
            .reserve_delegation_child(
                &actor,
                &session.id,
                &branch.id,
                1,
                limits,
                Some(&strategy.revision_id),
                1,
            )
            .unwrap();
        assert_eq!(
            root.child_routing_revision_id.as_deref(),
            Some(strategy.revision_id.as_str())
        );
        let route = store
            .routing_admit_resolved(
                &actor,
                &strategy.revision_id,
                &session.id,
                &branch.id,
                "worker",
                "subagents",
                &sha('a'),
                &sha('b'),
                RoutingSelectionSource::Fixed,
                "fast",
            )
            .unwrap();
        let worker = identity(&actor, "fast", "fast-model");
        let selected = store
            .routing_consume_for_delegation(&actor, &route.id, &allocation.id, &worker)
            .unwrap();
        assert_eq!(
            selected.routing_operation_id.as_deref(),
            Some(route.id.as_str())
        );
        assert_eq!(selected.routing_target_id.as_deref(), Some("fast"));
        assert_eq!(selected.routing_selection_source.as_deref(), Some("fixed"));
        assert_eq!(selected.selected_identity.as_ref(), Some(&worker));
        assert!(store
            .routing_consume_for_delegation(&actor, &route.id, &allocation.id, &worker)
            .is_err());

        let (relation, child, _) = store
            .create_subagent_session(
                &actor,
                &session.id,
                &branch.id,
                1,
                "routed child",
                &sha('c'),
                &worker,
                &root.id,
                &allocation.id,
                1,
                "off",
                None,
                SubagentMode::Foreground,
            )
            .unwrap();
        assert_eq!(relation.identity.account_id, "fast");
        assert_eq!(relation.identity.model, "fast-model");
        assert_eq!(child.binding.profile_hint, "fast");
        assert_eq!(child.binding.model_hint, "fast-model");
        assert_eq!(child.binding.destination_fingerprint, "a".repeat(64));
        let consumed = store.routing_operation(&actor, &route.id).unwrap();
        assert_eq!(consumed.working_identity.as_ref(), Some(&worker));
        assert!(consumed.consumed_at.is_some());
    }

    #[test]
    fn working_route_receipt_is_consumed_once_and_linked_to_llm_operation() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let (session, branch) = store.create(&actor, "route receipt", binding()).unwrap();
        let strategy = store
            .routing_strategy_define(&actor, strategy(RoutingMode::Fixed))
            .unwrap();
        let route = store
            .routing_admit_resolved(
                &actor,
                &strategy.revision_id,
                &session.id,
                &branch.id,
                "main",
                "main",
                &sha('a'),
                &sha('b'),
                RoutingSelectionSource::Fixed,
                "fast",
            )
            .unwrap();
        let (_, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        let working = identity(&actor, "fast", "fast-model");
        handle.bind_routing_operation(route.id.clone()).unwrap();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(handle.begin(TurnStart {
                turn_id: 1,
                prompt: "hello".into(),
                identity: working.clone(),
            }))
            .unwrap();

        let consumed = store.routing_operation(&actor, &route.id).unwrap();
        assert!(consumed.consumed_at.is_some());
        assert_eq!(consumed.working_identity, Some(working.clone()));
        let c = store.connection().unwrap();
        let linked: String = c
            .query_row(
                "SELECT routing_operation_id FROM llm_operations WHERE routing_operation_id=?1",
                [&route.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, route.id);
        assert!(
            consume_working_route_in(&c, &actor, &linked, &session.id, &branch, &working,).is_err()
        );
    }

    #[test]
    fn route_once_affinity_is_per_user_branch_lineage_and_cas() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let (session, branch) = store.create(&actor, "route", binding()).unwrap();
        let strategy = store
            .routing_strategy_define(&actor, strategy(RoutingMode::RouteOnce))
            .unwrap();
        let op = store
            .routing_admit_classifier(
                &actor,
                &strategy.revision_id,
                &session.id,
                &branch.id,
                "main",
                "main",
                &sha('a'),
                &sha('b'),
            )
            .unwrap();
        {
            let c = store.connection().unwrap();
            c.execute(
                "UPDATE routing_operations SET state='selected',selected_target_id='deep' WHERE id=?1",
                [&op.id],
            )
            .unwrap();
        }
        let affinity = store
            .routing_affinity_establish(&actor, &op.id, None)
            .unwrap();
        assert_eq!(affinity.target_id, "deep");
        assert_eq!(affinity.revision, 1);
        assert!(store
            .routing_affinity_establish(&actor, &op.id, None)
            .is_err());
        let fork = store
            .fork(&actor, &session.id, "fork", &branch.head)
            .unwrap();
        assert!(store
            .routing_affinity(&actor, &session.id, &fork.id, "main", "main")
            .unwrap()
            .is_none());
    }

    #[test]
    fn session_binding_precedes_user_binding_but_affinity_stays_per_actor() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let (session, branch) = store.create(&actor, "route", binding()).unwrap();
        let first = store
            .routing_strategy_define(&actor, strategy(RoutingMode::Fixed))
            .unwrap();
        store
            .routing_bind(
                &actor,
                &first.strategy_id,
                Some(&first.revision_id),
                RoutingBindingScope::User,
                "self",
                "main",
            )
            .unwrap();
        let mut other = strategy(RoutingMode::Fixed);
        other.name = "session-route".into();
        other.fixed_target = Some("deep".into());
        let second = store.routing_strategy_define(&actor, other).unwrap();
        store
            .routing_bind(
                &actor,
                &second.strategy_id,
                Some(&second.revision_id),
                RoutingBindingScope::Session,
                &session.id,
                "main",
            )
            .unwrap();
        let resolved = store
            .routing_resolve(&actor, None, None, &session.id, &branch.id, "main")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.strategy.strategy_id, second.strategy_id);
    }

    #[test]
    fn pre_dispatch_cancel_settles_only_unstarted_classifier_operation() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let (session, branch) = store.create(&actor, "route cancel", binding()).unwrap();
        let strategy = store
            .routing_strategy_define(&actor, strategy(RoutingMode::RouteEachPrompt))
            .unwrap();
        let op = store
            .routing_admit_classifier(
                &actor,
                &strategy.revision_id,
                &session.id,
                &branch.id,
                "main",
                "main",
                &sha('a'),
                &sha('b'),
            )
            .unwrap();
        assert!(store
            .routing_settle_unstarted(&actor, &op.id, true, "cancelled before dispatch")
            .unwrap());
        assert_eq!(
            store.routing_operation(&actor, &op.id).unwrap().state,
            "cancelled"
        );
        assert!(store.routing_attempt(&actor, &op.id).unwrap().is_none());
        assert!(!store
            .routing_settle_unstarted(&actor, &op.id, false, "must not rewrite")
            .unwrap());
    }

    #[test]
    fn abandoned_classifier_attempt_recovers_unknown_without_replay() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let (session, branch) = store.create(&actor, "route recovery", binding()).unwrap();
        let strategy = store
            .routing_strategy_define(&actor, strategy(RoutingMode::RouteEachPrompt))
            .unwrap();
        let op = store
            .routing_admit_classifier(
                &actor,
                &strategy.revision_id,
                &session.id,
                &branch.id,
                "main",
                "main",
                &sha('a'),
                &sha('b'),
            )
            .unwrap();
        let identity = InvocationIdentity {
            actor: actor.clone(),
            account_id: "router".into(),
            account_owner: aw_agent_core::AccountOwner::User(actor.clone()),
            payer: aw_agent_core::BillingOwner::User(actor.clone()),
            credential_custody: aw_agent_core::CredentialCustody::UserDevice("local".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "judge-model".into(),
            grant_revision: 1,
        };
        {
            let c = store.connection().unwrap();
            c.execute(
                "INSERT INTO local_process_incarnations(
                    process_id,store_id,store_generation,process_nonce,process_kind,os_pid,state,
                    started_at,last_seen_at,lease_expires_at,stopped_at)
                 SELECT 'dead-router',value,1,'dead-router-nonce','owner_lock',1,'stopped',1,1,NULL,2
                 FROM store_meta WHERE key='store_id'",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO routing_attempts(
                    id,operation_id,attempt_no,classifier_identity_json,state,owner_process_id,owner_store_generation,started_at)
                 VALUES('routeatt_dead',?1,1,?2,'started','dead-router',1,1)",
                params![op.id, json(&identity).unwrap()],
            )
            .unwrap();
            c.execute(
                "UPDATE routing_operations SET state='classifier_started' WHERE id=?1",
                [&op.id],
            )
            .unwrap();
        }

        store.recover().unwrap();

        let recovered = store.routing_operation(&actor, &op.id).unwrap();
        let attempt = store.routing_attempt(&actor, &op.id).unwrap().unwrap();
        assert_eq!(recovered.state, "unknown_external_effect");
        assert_eq!(attempt.state, "unknown_external_effect");
        assert!(attempt.error.unwrap().contains("no replay"));
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM routing_attempts WHERE operation_id=?1",
                    [&op.id],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn fixed_route_records_selection_without_classifier_attempt() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("unix:1000").unwrap();
        let (session, branch) = store.create(&actor, "route", binding()).unwrap();
        let strategy = store
            .routing_strategy_define(&actor, strategy(RoutingMode::Fixed))
            .unwrap();
        let op = store
            .routing_admit_resolved(
                &actor,
                &strategy.revision_id,
                &session.id,
                &branch.id,
                "main",
                "main",
                &sha('a'),
                &sha('b'),
                RoutingSelectionSource::Fixed,
                "fast",
            )
            .unwrap();
        assert_eq!(op.state, "selected");
        assert!(store.routing_attempt(&actor, &op.id).unwrap().is_none());
    }
}
