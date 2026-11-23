//! Durable source-owned Job event state for Agent/Turn resources.
//!
//! The shared aw-tools SourceEventBook owns rule replacement, subscription
//! generations and event revisions. This module only binds that pure state to
//! the authoritative SQLite Agent/Turn lifecycle and stores exact committed
//! deliveries until the existing Agent Job router attempts receiver admission.

use crate::{
    agent_binding_in, agent_owner, audit, db, now, session_in, Error, PrincipalId, Result,
    SessionParticipationAction, SessionStore,
};
use aw_store_api::AgentBindingInfo;
use aw_tools::job_event::{
    EventRuleSet, JobEventDelivery, JobEventKind, SourceEventBook, SubscriptionUpdate,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobWakeAdmission {
    pub recipient_ref: String,
    pub actor: String,
    pub authority_ref: String,
    pub session_id: String,
    pub membership_epoch: u64,
    pub audience_epoch: u64,
    pub owner: bool,
    /// Current opaque Agent-access dependency generation. It is verified at
    /// mint->persist time but need not be duplicated in the durable row because
    /// authority_ref cryptographically commits to it.
    pub access_generation: [u8; 32],
    pub tool_name: String,
    pub source_kind: String,
    pub source_id: String,
    pub source_epoch: u64,
    pub execution_site: String,
    pub visible: bool,
    pub registry_revision: Option<u64>,
    pub authority_revision: u64,
    pub effect: String,
}

impl AgentJobWakeAdmission {
    fn validate(&self) -> Result<()> {
        let bounded = |value: &str, max: usize| {
            !value.is_empty()
                && value.len() <= max
                && value.is_ascii()
                && !value.chars().any(char::is_control)
        };
        if !bounded(&self.recipient_ref, 2048)
            || !bounded(&self.actor, 256)
            || !bounded(&self.authority_ref, 256)
            || !bounded(&self.session_id, 256)
            || !bounded(&self.tool_name, 256)
            || !matches!(self.source_kind.as_str(), "host" | "mcp" | "extension")
            || !bounded(&self.source_id, 256)
            || !bounded(&self.execution_site, 256)
            || self.membership_epoch == 0
            || self.audience_epoch == 0
            || self.access_generation == [0; 32]
            || self.source_epoch == 0
            || self.authority_revision == 0
            || self.visible != self.registry_revision.is_some()
            || !matches!(
                self.effect.as_str(),
                "read" | "write" | "process" | "control" | "unknown"
            )
        {
            return Err(Error::Denied("invalid Agent Job wake admission receipt"));
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SourceState {
    book: SourceEventBook,
    last_result_turn: u64,
    settled: bool,
    pending: Vec<JobEventDelivery>,
}

fn canonical(binding: &AgentBindingInfo) -> String {
    format!(
        "/d:{}/i:{}/p:{}/a:{}",
        binding.domain_handle,
        binding.identity_handle,
        binding.project_handle,
        binding.agent_handle
    )
}

fn source_ref(binding: &AgentBindingInfo, turn: Option<u64>) -> String {
    let root = canonical(binding);
    turn.map_or(root.clone(), |turn| format!("{root}/t:{turn}"))
}

fn latest_attached_turn(c: &rusqlite::Connection, session: &str, branch: &str) -> Result<u64> {
    db(c.query_row(
        "SELECT COALESCE(MAX(turn_id),0) FROM runs
         WHERE session_id=?1 AND branch_id=?2
           AND status<>'running' AND attach_status='attached'",
        params![session, branch],
        |row| row.get(0),
    ))
}

fn turn_is_attached(
    c: &rusqlite::Connection,
    session: &str,
    branch: &str,
    turn: u64,
) -> Result<bool> {
    db(c.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM runs
            WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3
              AND status<>'running' AND attach_status='attached'
         )",
        params![session, branch, turn],
        |row| row.get(0),
    ))
}

fn row(c: &rusqlite::Connection, source: &str) -> Result<Option<SourceState>> {
    let encoded: Option<(String, u64, i64, String)> = db(c
        .query_row(
            "SELECT event_book_json,last_result_turn,settled,pending_deliveries_json
             FROM agent_job_event_sources WHERE source_ref=?1",
            [source],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional())?;
    encoded
        .map(|(book, last_result_turn, settled, pending)| {
            let book: SourceEventBook = serde_json::from_str(&book)
                .map_err(|_| Error::Storage("cannot decode Agent Job event source state"))?;
            book.validate().map_err(Error::Storage)?;
            let pending: Vec<JobEventDelivery> = serde_json::from_str(&pending)
                .map_err(|_| Error::Storage("cannot decode pending Agent Job events"))?;
            for delivery in &pending {
                delivery.validate().map_err(Error::Storage)?;
                if delivery.source_ref != source {
                    return Err(Error::Storage(
                        "pending Agent Job event belongs to another source",
                    ));
                }
            }
            Ok(SourceState {
                book,
                last_result_turn,
                settled: settled != 0,
                pending,
            })
        })
        .transpose()
}

fn save(
    c: &rusqlite::Connection,
    binding: &AgentBindingInfo,
    turn: Option<u64>,
    state: &SourceState,
) -> Result<()> {
    let source = source_ref(binding, turn);
    let book = serde_json::to_string(&state.book)
        .map_err(|_| Error::Storage("cannot encode Agent Job event source state"))?;
    let pending = serde_json::to_string(&state.pending)
        .map_err(|_| Error::Storage("cannot encode pending Agent Job events"))?;
    db(c.execute(
        "INSERT INTO agent_job_event_sources(
            source_ref,session_id,branch_id,turn_id,event_book_json,
            last_result_turn,settled,pending_deliveries_json,updated_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(source_ref) DO UPDATE SET
            event_book_json=excluded.event_book_json,
            last_result_turn=excluded.last_result_turn,
            settled=excluded.settled,
            pending_deliveries_json=excluded.pending_deliveries_json,
            updated_at=excluded.updated_at",
        params![
            source,
            binding.session_id,
            binding.branch_id,
            turn,
            book,
            state.last_result_turn,
            i64::from(state.settled),
            pending,
            now(),
        ],
    ))?;
    Ok(())
}

pub(crate) fn retire_recipient_in(c: &rusqlite::Connection, recipient_ref: &str) -> Result<usize> {
    let mut query = db(c.prepare(
        "SELECT source_ref,session_id,branch_id,event_book_json,pending_deliveries_json
         FROM agent_job_event_sources ORDER BY source_ref",
    ))?;
    let rows = db(query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    }))?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    drop(query);

    let mut affected_agents = BTreeSet::new();
    let mut changed_sources = 0usize;
    for (source, session, branch, encoded_book, encoded_pending) in rows {
        let mut book: SourceEventBook = serde_json::from_str(&encoded_book)
            .map_err(|_| Error::Storage("cannot decode Agent Job event source state"))?;
        book.validate().map_err(Error::Storage)?;
        let mut pending: Vec<JobEventDelivery> = serde_json::from_str(&encoded_pending)
            .map_err(|_| Error::Storage("cannot decode pending Agent Job events"))?;
        for delivery in &pending {
            delivery.validate().map_err(Error::Storage)?;
            if delivery.source_ref != source {
                return Err(Error::Storage(
                    "pending Agent Job event belongs to another source",
                ));
            }
        }

        let retired = book
            .retire_recipient(recipient_ref)
            .map_err(Error::Storage)?;
        let before_pending = pending.len();
        pending.retain(|delivery| delivery.recipient_ref != recipient_ref);
        if !retired.changed && pending.len() == before_pending {
            continue;
        }

        let book = serde_json::to_string(&book)
            .map_err(|_| Error::Storage("cannot encode Agent Job event source state"))?;
        let pending = serde_json::to_string(&pending)
            .map_err(|_| Error::Storage("cannot encode pending Agent Job events"))?;
        let changed = db(c.execute(
            "UPDATE agent_job_event_sources
             SET event_book_json=?1,pending_deliveries_json=?2,updated_at=?3
             WHERE source_ref=?4",
            params![book, pending, now(), source],
        ))?;
        if changed != 1 {
            return Err(Error::Storage(
                "Agent Job event source disappeared during recipient retirement",
            ));
        }
        affected_agents.insert((session, branch));
        changed_sources += 1;
    }

    db(c.execute(
        "DELETE FROM agent_job_wake_admissions WHERE recipient_ref=?1",
        [recipient_ref],
    ))?;
    for (session, branch) in affected_agents {
        agent_owner::changed(c, agent_owner::AgentKey::new(&session, &branch))?;
    }
    Ok(changed_sources)
}

fn agent_is_settled(c: &rusqlite::Connection, session: &str) -> Result<bool> {
    let state: Option<String> = db(c
        .query_row(
            "SELECT state FROM subagent_sessions
             WHERE child_session_id=?1
             ORDER BY created_at DESC LIMIT 1",
            [session],
            |row| row.get(0),
        )
        .optional())?;
    Ok(state.is_some_and(|state| state != "running"))
}

fn initial_state(
    c: &rusqlite::Connection,
    binding: &AgentBindingInfo,
    turn: Option<u64>,
) -> Result<SourceState> {
    let turn_settled = turn
        .map(|turn| turn_is_attached(c, &binding.session_id, &binding.branch_id, turn))
        .transpose()?
        .unwrap_or(false);
    let source = source_ref(binding, turn);
    let next_event_rev = crate::event_source::next_event_revision(c, &source)?;
    Ok(SourceState {
        book: SourceEventBook::from_next_event_revision(next_event_rev).map_err(Error::Storage)?,
        last_result_turn: match turn {
            Some(turn) if turn_settled => turn,
            Some(_) => 0,
            None => latest_attached_turn(c, &binding.session_id, &binding.branch_id)?,
        },
        settled: match turn {
            Some(_) => turn_settled,
            None => agent_is_settled(c, &binding.session_id)?,
        },
        pending: Vec::new(),
    })
}

fn commit_source(
    c: &rusqlite::Connection,
    binding: &AgentBindingInfo,
    turn: Option<u64>,
    events: &[JobEventKind],
    result_turn: Option<u64>,
    settle: bool,
    actor: &PrincipalId,
) -> Result<()> {
    let source = source_ref(binding, turn);
    let mut state = row(c, &source)?;
    if let Some(state) = state.as_mut() {
        if let Some(result_turn) = result_turn {
            if result_turn <= state.last_result_turn {
                return Ok(());
            }
            state.last_result_turn = result_turn;
        }
        if settle {
            if state.settled {
                return Ok(());
            }
            state.settled = true;
        }
    }

    let legacy_revision = if let Some(state) = state.as_mut() {
        let deliveries = state.book.commit(&source, events).map_err(Error::Storage)?;
        let revision = state.book.current_event_revision();
        state.pending.extend(deliveries);
        Some(revision)
    } else {
        None
    };
    let system_events = events
        .iter()
        .map(|event| crate::event_source::SystemEvent::new(event.as_str()))
        .collect::<Vec<_>>();
    let event_revision = crate::event_source::emit_system_events(
        c,
        &source,
        actor,
        result_turn.or(turn),
        &system_events,
        legacy_revision,
    )?;

    if let Some(state) = state {
        save(c, binding, turn, &state)?;
        audit(
            c,
            &binding.session_id,
            actor,
            "agent.job.event",
            serde_json::json!({
                "source_ref": source,
                "turn_id": turn,
                "result_turn": result_turn,
                "event_rev": event_revision,
                "events": events.iter().map(|event| event.as_str()).collect::<Vec<_>>(),
            }),
        )?;
    }
    Ok(())
}

pub(crate) fn commit_turn_settled_in(
    c: &rusqlite::Connection,
    session: &str,
    branch: &str,
    turn: u64,
    actor: &PrincipalId,
) -> Result<()> {
    let owner = session_in(c, session)?.owner;
    let binding = agent_binding_in(c, &owner, session, branch)?;
    commit_source(
        c,
        &binding,
        None,
        &[JobEventKind::Result],
        Some(turn),
        false,
        actor,
    )?;
    commit_source(
        c,
        &binding,
        Some(turn),
        &[JobEventKind::Result, JobEventKind::Settled],
        Some(turn),
        true,
        actor,
    )
}

pub(crate) fn commit_agent_settled_in(
    c: &rusqlite::Connection,
    session: &str,
    branch: &str,
    actor: &PrincipalId,
) -> Result<()> {
    let binding = agent_binding_in(c, actor, session, branch)?;
    commit_source(
        c,
        &binding,
        None,
        &[JobEventKind::Settled],
        None,
        true,
        actor,
    )
}

impl SessionStore {
    pub fn store_agent_job_wake_admission(&self, receipt: AgentJobWakeAdmission) -> Result<()> {
        receipt.validate()?;
        let actor = PrincipalId::new(receipt.actor.clone())?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let access = agent_owner::key_for_ref(&tx, &receipt.recipient_ref).and_then(
            |(session, branch)| {
                agent_owner::authorize_access(
                    &tx,
                    &actor,
                    agent_owner::AgentKey::new(&session, &branch),
                    SessionParticipationAction::Control,
                )
            },
        )?;
        if access.grant_revision != receipt.membership_epoch
            || access.acl_revision != receipt.audience_epoch
            || access.owner != receipt.owner
            || access.access_generation != receipt.access_generation
        {
            return Err(Error::Denied(
                "Agent Job wake admission access changed before persistence",
            ));
        }
        db(tx.execute(
            "INSERT INTO agent_job_wake_admissions(
                recipient_ref,actor,authority_ref,session_id,membership_epoch,audience_epoch,
                is_owner,tool_name,source_kind,source_id,source_epoch,execution_site,
                is_visible,registry_revision,authority_revision,effect,updated_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(recipient_ref,actor) DO UPDATE SET
                authority_ref=excluded.authority_ref,
                session_id=excluded.session_id,
                membership_epoch=excluded.membership_epoch,
                audience_epoch=excluded.audience_epoch,
                is_owner=excluded.is_owner,
                tool_name=excluded.tool_name,
                source_kind=excluded.source_kind,
                source_id=excluded.source_id,
                source_epoch=excluded.source_epoch,
                execution_site=excluded.execution_site,
                is_visible=excluded.is_visible,
                registry_revision=excluded.registry_revision,
                authority_revision=excluded.authority_revision,
                effect=excluded.effect,
                updated_at=excluded.updated_at",
            params![
                receipt.recipient_ref,
                receipt.actor,
                receipt.authority_ref,
                receipt.session_id,
                receipt.membership_epoch,
                receipt.audience_epoch,
                i64::from(receipt.owner),
                receipt.tool_name,
                receipt.source_kind,
                receipt.source_id,
                receipt.source_epoch,
                receipt.execution_site,
                i64::from(receipt.visible),
                receipt.registry_revision,
                receipt.authority_revision,
                receipt.effect,
                now(),
            ],
        ))?;
        db(tx.commit())?;
        Ok(())
    }

    pub fn agent_job_wake_admission(
        &self,
        authority_ref: &str,
    ) -> Result<Option<AgentJobWakeAdmission>> {
        if authority_ref.is_empty()
            || authority_ref.len() > 256
            || !authority_ref.is_ascii()
            || authority_ref.chars().any(char::is_control)
        {
            return Err(Error::Denied("invalid Agent Job wake admission identity"));
        }
        let connection = self.connection()?;
        let mut receipt = db(connection
            .query_row(
                "SELECT recipient_ref,actor,authority_ref,session_id,membership_epoch,
                        audience_epoch,is_owner,tool_name,source_kind,source_id,source_epoch,
                        execution_site,is_visible,registry_revision,authority_revision,effect
                 FROM agent_job_wake_admissions WHERE authority_ref=?1",
                [authority_ref],
                |row| {
                    Ok(AgentJobWakeAdmission {
                        recipient_ref: row.get(0)?,
                        actor: row.get(1)?,
                        authority_ref: row.get(2)?,
                        session_id: row.get(3)?,
                        membership_epoch: row.get(4)?,
                        audience_epoch: row.get(5)?,
                        owner: row.get::<_, i64>(6)? != 0,
                        access_generation: [0; 32],
                        tool_name: row.get(7)?,
                        source_kind: row.get(8)?,
                        source_id: row.get(9)?,
                        source_epoch: row.get(10)?,
                        execution_site: row.get(11)?,
                        visible: row.get::<_, i64>(12)? != 0,
                        registry_revision: row.get(13)?,
                        authority_revision: row.get(14)?,
                        effect: row.get(15)?,
                    })
                },
            )
            .optional())?;
        if let Some(receipt) = &mut receipt {
            let actor = PrincipalId::new(receipt.actor.clone())?;
            let (session, branch) = agent_owner::key_for_ref(&connection, &receipt.recipient_ref)?;
            let access = agent_owner::authorize_access(
                &connection,
                &actor,
                agent_owner::AgentKey::new(&session, &branch),
                SessionParticipationAction::Control,
            )?;
            receipt.access_generation = access.access_generation;
            receipt.validate()?;
        }
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn replace_agent_job_subscription(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        turn: Option<u64>,
        expected_revision: Option<u64>,
        recipient_ref: &str,
        recipient_authority_ref: &str,
        rules: EventRuleSet,
        capacity: usize,
    ) -> Result<(String, SubscriptionUpdate)> {
        if capacity == 0 || capacity > 8_192 {
            return Err(Error::Limit("invalid Agent Job subscription capacity"));
        }
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let binding = agent_binding_in(&tx, actor, session, branch)?;
        agent_owner::authorize_access(
            &tx,
            actor,
            agent_owner::AgentKey::new(session, &binding.branch_id),
            SessionParticipationAction::Control,
        )?;
        if let Some(expected) = expected_revision {
            if agent_owner::revision(&tx, agent_owner::AgentKey::new(session, &binding.branch_id))?
                != expected
            {
                return Err(Error::Conflict("stale Agent revision"));
            }
        }
        if let Some(turn) = turn {
            let exists: bool = db(tx.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM runs
                    WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3
                 )",
                params![session, branch, turn],
                |row| row.get(0),
            ))?;
            if !exists {
                return Err(Error::Denied("Turn subscription source is unavailable"));
            }
        }
        let source = source_ref(&binding, turn);
        let existing = row(&tx, &source)?;
        if existing.is_none() && rules.is_empty() {
            db(tx.commit())?;
            return Ok((
                source,
                SubscriptionUpdate {
                    generation: 0,
                    changed: false,
                    active: false,
                },
            ));
        }
        let mut state = match existing {
            Some(state) => state,
            None => initial_state(&tx, &binding, turn)?,
        };
        if state.settled && !rules.is_empty() {
            return Err(Error::Conflict(
                "Agent Job subscription source is already settled",
            ));
        }
        if state.book.generation_for(recipient_ref).is_none()
            && !rules.is_empty()
            && state.book.subscription_count() >= capacity
        {
            return Err(Error::Limit("Agent Job subscription capacity exhausted"));
        }
        let update = state
            .book
            .replace(recipient_ref, recipient_authority_ref, rules)
            .map_err(Error::Denied)?;
        if update.changed {
            save(&tx, &binding, turn, &state)?;
            audit(
                &tx,
                session,
                actor,
                "agent.job.subscription",
                serde_json::json!({
                    "source_ref": source,
                    "recipient_ref": recipient_ref,
                    "generation": update.generation,
                    "active": update.active,
                }),
            )?;
            agent_owner::changed(&tx, agent_owner::AgentKey::new(session, &binding.branch_id))?;
        }
        db(tx.commit())?;
        Ok((source, update))
    }

    pub fn pending_agent_job_events(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<Vec<JobEventDelivery>> {
        let connection = self.connection()?;
        let binding = agent_binding_in(&connection, actor, session, branch)?;
        let root = canonical(&binding);
        let mut query = db(connection.prepare(
            "SELECT source_ref,pending_deliveries_json
             FROM agent_job_event_sources
             WHERE session_id=?1 AND branch_id=?2 AND pending_deliveries_json<>'[]'
             ORDER BY CASE WHEN source_ref=?3 THEN 0 ELSE 1 END, source_ref",
        ))?;
        let rows = db(query.query_map(params![session, branch, root], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }))?;
        let mut out = Vec::new();
        for row in rows {
            let (source, encoded) = db(row)?;
            let deliveries: Vec<JobEventDelivery> = serde_json::from_str(&encoded)
                .map_err(|_| Error::Storage("cannot decode pending Agent Job events"))?;
            for delivery in deliveries {
                delivery.validate().map_err(Error::Storage)?;
                if delivery.source_ref != source {
                    return Err(Error::Storage(
                        "pending Agent Job event belongs to another source",
                    ));
                }
                out.push(delivery);
            }
        }
        Ok(out)
    }

    pub fn ack_agent_job_event(
        &self,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        delivery: &JobEventDelivery,
    ) -> Result<()> {
        delivery.validate().map_err(Error::Denied)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let binding = agent_binding_in(&tx, actor, session, branch)?;
        let root = canonical(&binding);
        if delivery.source_ref != root
            && !delivery
                .source_ref
                .strip_prefix(&format!("{root}/t:"))
                .is_some_and(|turn| {
                    turn.parse::<u64>()
                        .ok()
                        .is_some_and(|value| value != 0 && value.to_string() == turn)
                })
        {
            return Err(Error::Denied(
                "Agent Job event ACK belongs to another source",
            ));
        }
        let Some(mut state) = row(&tx, &delivery.source_ref)? else {
            db(tx.commit())?;
            return Ok(());
        };
        let before = state.pending.len();
        state.pending.retain(|candidate| candidate != delivery);
        if state.pending.len() != before {
            let turn = delivery
                .source_ref
                .strip_prefix(&format!("{root}/t:"))
                .and_then(|value| value.parse::<u64>().ok());
            save(&tx, &binding, turn, &state)?;
        }
        db(tx.commit())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, DeltaSink, Message, Outcome, Provider, Turn,
        TurnJournal, TurnStart,
    };
    use aw_store_api::SessionBinding;
    use aw_tools::job_event::{EventRule, JobWakeMode};
    use std::{path::PathBuf, sync::Arc};

    struct UnusedProvider;

    impl Provider for UnusedProvider {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            _sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { Err(aw_agent_core::Error::Protocol("unused provider")) })
        }
    }

    fn binding() -> SessionBinding {
        SessionBinding {
            working_directory: PathBuf::from("/tmp"),
            tool_root: PathBuf::from("/tmp"),
            system_prompt: String::new(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[tokio::test]
    async fn attached_turn_commits_root_and_exact_turn_events_until_exact_ack() {
        let store = SessionStore::in_memory().unwrap();
        let agent = Agent::builder(
            Arc::new(UnusedProvider),
            AgentOptions::new("fixture", "model"),
        )
        .build()
        .unwrap();
        let actor = agent.actor().clone();
        let identity = agent.default_identity().unwrap();
        let (session, branch) = store.create(&actor, "events", binding()).unwrap();
        let (_, journal) = store.activate(&actor, &session.id, &branch.id).unwrap();
        journal
            .begin(TurnStart {
                turn_id: 1,
                prompt: "event source".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();

        let recipient = "/d:0/i:0/p:0/a:99";
        let authority = "jwa_durable_fixture";
        let root_rules =
            EventRuleSet::new([EventRule::new(JobEventKind::Result, JobWakeMode::Queue)]).unwrap();
        let (root, root_update) = store
            .replace_agent_job_subscription(
                &actor,
                &session.id,
                &branch.id,
                None,
                None,
                recipient,
                authority,
                root_rules,
                8,
            )
            .unwrap();
        assert_eq!(root_update.generation, 1);
        let revision = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        let (_, unchanged) = store
            .replace_agent_job_subscription(
                &actor,
                &session.id,
                &branch.id,
                None,
                Some(revision),
                recipient,
                authority,
                EventRuleSet::new([EventRule::new(JobEventKind::Result, JobWakeMode::Queue)])
                    .unwrap(),
                8,
            )
            .unwrap();
        assert!(!unchanged.changed);
        assert_eq!(unchanged.generation, root_update.generation);
        assert_eq!(
            store
                .agent_public_revision(&actor, &session.id, &branch.id)
                .unwrap(),
            revision,
            "idempotent subscription replacement must not advance Agent revision"
        );

        let turn_rules = EventRuleSet::new([
            EventRule::new(JobEventKind::Result, JobWakeMode::Queue),
            EventRule::new(JobEventKind::Settled, JobWakeMode::Queue),
        ])
        .unwrap();
        let (turn_ref, turn_update) = store
            .replace_agent_job_subscription(
                &actor,
                &session.id,
                &branch.id,
                Some(1),
                None,
                recipient,
                authority,
                turn_rules,
                8,
            )
            .unwrap();
        assert_eq!(turn_ref, format!("{root}/t:1"));
        assert_eq!(turn_update.generation, 1);
        assert!(store
            .pending_agent_job_events(&actor, &session.id, &branch.id)
            .unwrap()
            .is_empty());

        let root_subject =
            aw_tools::event_subject::EventSubject::from_resource_ref(&root, "result")
                .unwrap()
                .into_string();
        let root_subject_prefix = root_subject
            .strip_suffix(".e:result")
            .expect("concrete Event subject has final event token");
        let read_pattern = format!("{root_subject_prefix}.>");
        let project_subject_prefix = root_subject_prefix
            .rsplit_once(".a:")
            .expect("Agent Event subject has final Agent token")
            .0;
        let project_pattern = format!("{project_subject_prefix}.a:*.>");
        let subscription = store
            .events_subscribe(&actor, &project_pattern, &root, true)
            .unwrap();
        assert!(subscription.enabled);
        assert!(subscription.changed);
        assert_eq!(subscription.generation, Some(1));
        assert_eq!(
            subscription.state,
            Some(crate::EventSubscriptionState::Active)
        );
        let subscription_floor = {
            let connection = store.connection().unwrap();
            connection
                .query_row(
                    "SELECT baseline_position,scan_position,created_at,updated_at
                     FROM event_subscriptions_current
                     WHERE recipient_ref=?1 AND subject=?2",
                    params![&root, &project_pattern],
                    |row| {
                        Ok((
                            row.get::<_, u64>(0)?,
                            row.get::<_, u64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
                .unwrap()
        };
        assert_eq!(subscription_floor.0, subscription_floor.1);
        let unchanged_subscription = store
            .events_subscribe(&actor, &project_pattern, &root, true)
            .unwrap();
        assert!(!unchanged_subscription.changed);
        assert_eq!(unchanged_subscription.generation, Some(1));
        let unchanged_floor = {
            let connection = store.connection().unwrap();
            connection
                .query_row(
                    "SELECT baseline_position,scan_position,created_at,updated_at
                     FROM event_subscriptions_current
                     WHERE recipient_ref=?1 AND subject=?2",
                    params![&root, &project_pattern],
                    |row| {
                        Ok((
                            row.get::<_, u64>(0)?,
                            row.get::<_, u64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
                .unwrap()
        };
        assert_eq!(
            unchanged_floor, subscription_floor,
            "active enabled=true must be a zero-write no-op"
        );
        let listed_subscriptions = store
            .events_subscriptions_list(&actor, &root, None, 16)
            .unwrap();
        assert_eq!(listed_subscriptions.len(), 1);
        assert_eq!(listed_subscriptions[0].subject, project_pattern);
        assert_eq!(listed_subscriptions[0].generation, 1);
        assert_eq!(
            listed_subscriptions[0].state,
            crate::EventSubscriptionState::Active
        );
        let public_subscription = serde_json::to_string(&listed_subscriptions[0]).unwrap();
        assert!(!public_subscription.contains("baseline"));
        assert!(!public_subscription.contains("scan_position"));
        assert!(!public_subscription.contains("placement_token"));

        let owner_project_cursor = match store
            .events_read_page(&actor, &project_pattern, crate::EventReadAfter::Now, 16)
            .unwrap()
        {
            crate::EventReadOutcome::Page(page) => {
                assert!(page.events.is_empty());
                page.cursor
            }
            other => panic!("unexpected Project Event cursor outcome: {other:?}"),
        };
        let owner_cursor = match store
            .events_read_page(&actor, &read_pattern, crate::EventReadAfter::Now, 16)
            .unwrap()
        {
            crate::EventReadOutcome::Page(page) => {
                assert!(page.events.is_empty());
                page.cursor
            }
            other => panic!("unexpected owner Event cursor outcome: {other:?}"),
        };
        let owner_wait = {
            let store = store.clone();
            let actor = actor.clone();
            let read_pattern = read_pattern.clone();
            let cursor = owner_cursor.clone();
            tokio::spawn(async move {
                store
                    .events_read(
                        &actor,
                        &read_pattern,
                        crate::EventReadAfter::Cursor(cursor),
                        16,
                        5_000,
                    )
                    .await
            })
        };
        tokio::task::yield_now().await;
        assert!(
            !owner_wait.is_finished(),
            "empty Event wait must remain pending before publication"
        );

        let observer = PrincipalId::new("internal:fixture:event-observer").unwrap();
        let observer_ref = {
            let connection = store.connection().unwrap();
            let observer_user = crate::ensure_user_for_principal(&connection, &observer).unwrap();
            let domain = root
                .strip_prefix("/d:")
                .and_then(|value| value.split('/').next())
                .expect("canonical Agent ref has Domain");
            format!("/d:{domain}/i:{observer_user}")
        };
        let revision = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        store
            .agent_access_set(
                &actor,
                &session.id,
                &branch.id,
                &observer_ref,
                Some(crate::AgentPermissions {
                    view: true,
                    ..Default::default()
                }),
                revision,
            )
            .unwrap();
        assert!(matches!(
            store.events_read_page(&observer, &project_pattern, crate::EventReadAfter::Now, 16,),
            Err(Error::Denied(_))
        ));
        let observer_cursor = match store
            .events_read_page(&observer, &read_pattern, crate::EventReadAfter::Now, 16)
            .unwrap()
        {
            crate::EventReadOutcome::Page(page) => {
                assert!(page.events.is_empty());
                page.cursor
            }
            other => panic!("unexpected observer Event cursor outcome: {other:?}"),
        };

        journal
            .finish(Turn {
                id: 1,
                prompt: "event source".into(),
                answer: "done".into(),
                model: identity.model.clone(),
                profile: identity.account_id.clone(),
                outcome: Outcome::Completed,
                meta: None,
                error: None,
                actor: Some(actor.clone()),
                tool_records: Vec::new(),
                invocations: Vec::new(),
                context_messages: Vec::new(),
                needs_reconciliation: false,
            })
            .await
            .unwrap();

        let pending = store
            .pending_agent_job_events(&actor, &session.id, &branch.id)
            .unwrap();
        assert_eq!(pending.len(), 3);
        let root_result = pending
            .iter()
            .find(|delivery| delivery.source_ref == root)
            .unwrap();
        assert_eq!(root_result.event, JobEventKind::Result);
        assert_eq!(root_result.event_rev, 1);

        let turn_events = pending
            .iter()
            .filter(|delivery| delivery.source_ref == turn_ref)
            .collect::<Vec<_>>();
        assert_eq!(turn_events.len(), 2);
        assert_eq!(turn_events[0].event_rev, turn_events[1].event_rev);
        assert_eq!(turn_events[0].event_rev, 1);
        assert!(turn_events
            .iter()
            .any(|delivery| delivery.event == JobEventKind::Result));
        assert!(turn_events
            .iter()
            .any(|delivery| delivery.event == JobEventKind::Settled));

        {
            let connection = store.connection().unwrap();
            for source in [&root, &turn_ref] {
                assert_eq!(
                    crate::event_source::next_event_revision(&connection, source).unwrap(),
                    2,
                    "legacy and DL-200 source revisions must stay aligned"
                );
            }
            let mut query = connection
                .prepare(
                    "SELECT p.source_ref,p.subject,p.event_rev,p.event_id,
                            json_extract(c.payload_json,'$._aw_dl200_event.actor'),
                            json_extract(c.payload_json,'$._aw_dl200_event.turn')
                     FROM event_projection_index p
                     JOIN commit_log c ON c.seq=p.commit_position
                     WHERE p.source_ref IN (?1,?2)
                     ORDER BY p.commit_position",
                )
                .unwrap();
            let projected = query
                .query_map(params![&root, &turn_ref], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, u64>(5)?,
                    ))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert_eq!(projected.len(), 3);
            let root_subject =
                aw_tools::event_subject::EventSubject::from_resource_ref(&root, "result")
                    .unwrap()
                    .into_string();
            assert_eq!(
                projected
                    .iter()
                    .filter(|event| event.0 == root)
                    .map(|event| (event.1.clone(), event.2))
                    .collect::<Vec<_>>(),
                vec![(root_subject, 1)]
            );
            let turn_projected = projected
                .iter()
                .filter(|event| event.0 == turn_ref)
                .collect::<Vec<_>>();
            assert_eq!(turn_projected.len(), 2);
            assert!(turn_projected.iter().all(|event| event.2 == 1));
            assert!(turn_projected.iter().all(|event| !event.3.is_empty()));
            assert!(projected.iter().all(|event| event.4 == actor.as_str()));
            assert!(projected.iter().all(|event| event.5 == 1));
        }

        let owner_page = match owner_wait.await.unwrap().unwrap() {
            crate::EventReadOutcome::Page(page) => page,
            other => panic!("unexpected owner Event wait outcome: {other:?}"),
        };
        let project_page = match store
            .events_read_page(
                &actor,
                &project_pattern,
                crate::EventReadAfter::Cursor(owner_project_cursor),
                16,
            )
            .unwrap()
        {
            crate::EventReadOutcome::Page(page) => page,
            other => panic!("unexpected Project Event read outcome: {other:?}"),
        };
        assert_eq!(project_page.events.len(), 3);
        assert_eq!(
            project_page
                .events
                .iter()
                .map(|event| event.subject.as_str())
                .collect::<Vec<_>>(),
            owner_page
                .events
                .iter()
                .map(|event| event.subject.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(owner_page.cursor, owner_cursor);
        assert_eq!(owner_page.events.len(), 3);
        assert!(owner_page.events.iter().all(|event| event.event_rev == 1));
        assert!(owner_page
            .events
            .iter()
            .all(|event| event.actor.as_deref() == Some(actor.as_str())));
        assert!(owner_page.events.iter().all(|event| event.turn == Some(1)));
        assert!(owner_page.events.iter().all(|event| event.causal_hop == 0));
        assert_eq!(
            owner_page
                .events
                .iter()
                .map(|event| event.subject.as_str())
                .collect::<Vec<_>>(),
            vec![
                root_subject.as_str(),
                aw_tools::event_subject::EventSubject::from_resource_ref(&turn_ref, "result")
                    .unwrap()
                    .as_str(),
                aw_tools::event_subject::EventSubject::from_resource_ref(&turn_ref, "settled")
                    .unwrap()
                    .as_str(),
            ]
        );
        let owner_empty = match store
            .events_read(
                &actor,
                &read_pattern,
                crate::EventReadAfter::Cursor(owner_page.cursor.clone()),
                16,
                1,
            )
            .await
            .unwrap()
        {
            crate::EventReadOutcome::Page(page) => page,
            other => panic!("unexpected timed owner Event read outcome: {other:?}"),
        };
        assert!(owner_empty.events.is_empty());

        let disabled_subscription = store
            .events_subscribe(&actor, &project_pattern, &root, false)
            .unwrap();
        assert!(!disabled_subscription.enabled);
        assert!(disabled_subscription.changed);
        assert_eq!(disabled_subscription.generation, Some(1));
        let reenabled_subscription = store
            .events_subscribe(&actor, &project_pattern, &root, true)
            .unwrap();
        assert!(reenabled_subscription.enabled);
        assert!(reenabled_subscription.changed);
        assert_eq!(reenabled_subscription.generation, Some(2));
        let rebaseline = {
            let connection = store.connection().unwrap();
            connection
                .query_row(
                    "SELECT generation,baseline_position,scan_position,state
                     FROM event_subscriptions_current
                     WHERE recipient_ref=?1 AND subject=?2",
                    params![&root, &project_pattern],
                    |row| {
                        Ok((
                            row.get::<_, u64>(0)?,
                            row.get::<_, u64>(1)?,
                            row.get::<_, u64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .unwrap()
        };
        assert_eq!(rebaseline.0, 2);
        assert_eq!(rebaseline.1, rebaseline.2);
        assert!(rebaseline.1 > subscription_floor.0);
        assert_eq!(rebaseline.3, "active");

        let revision = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        let revoked_revision = store
            .agent_access_set(
                &actor,
                &session.id,
                &branch.id,
                &observer_ref,
                None,
                revision,
            )
            .unwrap();
        let hidden_page = match store
            .events_read_page(
                &observer,
                &read_pattern,
                crate::EventReadAfter::Cursor(observer_cursor.clone()),
                16,
            )
            .unwrap()
        {
            crate::EventReadOutcome::Page(page) => page,
            other => panic!("unexpected hidden observer Event read outcome: {other:?}"),
        };
        assert!(hidden_page.events.is_empty());
        store
            .agent_access_set(
                &actor,
                &session.id,
                &branch.id,
                &observer_ref,
                Some(crate::AgentPermissions {
                    view: true,
                    ..Default::default()
                }),
                revoked_revision,
            )
            .unwrap();
        let regained_page = match store
            .events_read_page(
                &observer,
                &read_pattern,
                crate::EventReadAfter::Cursor(hidden_page.cursor),
                16,
            )
            .unwrap()
        {
            crate::EventReadOutcome::Page(page) => page,
            other => panic!("unexpected regained observer Event read outcome: {other:?}"),
        };
        assert!(
            regained_page.events.is_empty(),
            "regaining view cannot reveal Events already privately skipped while denied"
        );

        store
            .ack_agent_job_event(&actor, &session.id, &branch.id, root_result)
            .unwrap();
        assert_eq!(
            store
                .pending_agent_job_events(&actor, &session.id, &branch.id)
                .unwrap()
                .len(),
            2
        );

        let before_retire = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        let mut connection = store.connection().unwrap();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(retire_recipient_in(&tx, recipient).unwrap(), 2);
        tx.commit().unwrap();
        drop(connection);
        assert!(store
            .pending_agent_job_events(&actor, &session.id, &branch.id)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .agent_public_revision(&actor, &session.id, &branch.id)
                .unwrap(),
            before_retire + 1,
            "retiring one receiver across root+Turn sources advances the source Agent once"
        );
        let (_, already_retired) = store
            .replace_agent_job_subscription(
                &actor,
                &session.id,
                &branch.id,
                None,
                None,
                recipient,
                authority,
                EventRuleSet::default(),
                8,
            )
            .unwrap();
        assert!(!already_retired.changed);

        let closing = store.agent_request_close(&actor, &root, None).unwrap();
        assert_eq!(closing.lifecycle, crate::AgentLifecycle::Closing);
        let close_disabled = store
            .events_subscribe(&actor, &project_pattern, &root, false)
            .unwrap();
        assert!(!close_disabled.enabled);
        assert!(!close_disabled.changed);
        assert_eq!(close_disabled.generation, Some(2));
        assert!(store
            .events_subscriptions_list(&actor, &root, None, 16)
            .unwrap()
            .is_empty());

        let mut connection = store.connection().unwrap();
        let commit_count: u64 = connection
            .query_row("SELECT count(*) FROM commit_log", [], |row| row.get(0))
            .unwrap();
        connection
            .execute_batch(
                "DROP TABLE event_subscriptions_current;
                 DROP TABLE event_read_cursors_current;
                 DROP TABLE event_sources_current;
                 UPDATE store_meta SET value='44' WHERE key='schema_epoch';
                 UPDATE schema_meta
                    SET schema_epoch=44,activation_epoch=44,migration_phase='ready'
                    WHERE singleton=1;
                 UPDATE migration_lease
                    SET holder_client_id=NULL,current_epoch=44,target_epoch=NULL,lease_expires_at=NULL
                    WHERE singleton=1;
                 PRAGMA user_version=44;",
            )
            .unwrap();
        crate::migrate_event_sources(&mut connection).unwrap();
        crate::migrate_event_read_cursors(&mut connection).unwrap();
        crate::migrate_event_read_placement(&mut connection).unwrap();
        crate::migrate_event_subscriptions(&mut connection).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            48
        );
        for source in [&root, &turn_ref] {
            assert_eq!(
                crate::event_source::next_event_revision(&connection, source).unwrap(),
                2,
                "v44 -> v45 migration must recover the first unissued source revision"
            );
        }
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM commit_log", [], |row| row
                    .get::<_, u64>(0))
                .unwrap(),
            commit_count,
            "source-state migration must not rewrite immutable Event history"
        );
    }
}
