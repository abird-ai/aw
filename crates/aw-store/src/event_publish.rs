//! Custom DL-200 Event publication.
//!
//! Host publication acts as one exact open Agent under current control.
//! Turn publication is a lower trusted-runtime seam: the caller may expose it
//! to a model only when the current Turn carries the exact events_publish
//! capability. This module does not manufacture that capability.

use crate::{
    agent_owner, db, decode, event_authority, event_source, history_owner, now, Error, PrincipalId,
    Result, SessionHandle, SessionStore,
};
use aw_agent_core::{
    EventKey, EventPayload, EventSchemaRef, EventTypeRegistryReader, InvocationIdentity,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

const MAX_MODEL_CUSTOM_EVENTS_PER_TURN: u64 = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventPublished {
    pub subject: String,
    pub event_rev: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causal_root: Option<String>,
    pub causal_hop: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<EventSchemaRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "result")]
pub enum EventPublishOutcome {
    Published(EventPublished),
    CausalExhausted { event_rev: u64, causal_root: String },
}

impl SessionStore {
    pub fn events_publish(
        &self,
        actor: &PrincipalId,
        agent_ref: &str,
        event: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<EventPublishOutcome> {
        self.events_publish_host_inner(actor, agent_ref, event, data, None)
    }

    pub fn events_publish_typed<T: EventPayload>(
        &self,
        registry: &EventTypeRegistryReader,
        key: &EventKey<T>,
        actor: &PrincipalId,
        agent_ref: &str,
        value: &T,
    ) -> Result<EventPublishOutcome> {
        let descriptor = registry.revalidate(key)?;
        let data = serde_json::to_value(value)
            .map_err(|_| Error::Denied("typed Event payload cannot be encoded"))?;
        event_authority::validate_data(&data)?;
        let schema = key.schema();
        debug_assert_eq!(descriptor.event, key.event());
        self.events_publish_host_inner(actor, agent_ref, key.event(), Some(&data), Some(&schema))
    }

    fn events_publish_host_inner(
        &self,
        actor: &PrincipalId,
        agent_ref: &str,
        event: &str,
        data: Option<&serde_json::Value>,
        schema: Option<&EventSchemaRef>,
    ) -> Result<EventPublishOutcome> {
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (session, branch) = agent_owner::key_for_ref(&tx, agent_ref)?;
        let agent = agent_owner::AgentKey::new(&session, &branch);
        let (lifecycle, _) = agent_owner::lifecycle(&tx, agent)?;
        if lifecycle != "open" {
            return Err(Error::Conflict(
                "custom Event publication requires an open Agent",
            ));
        }
        agent_owner::authorize_permission(
            &tx,
            actor,
            agent,
            agent_owner::AgentPermission::Control,
        )?;
        let canonical = agent_owner::canonical_ref(&tx, agent)?;
        if canonical != agent_ref {
            return Err(Error::Denied(
                "custom Event source must be one exact canonical Agent",
            ));
        }
        let manifest = history_owner::event_publication_manifest_for_agent(&tx, &session, &branch)?;
        let outcome = event_source::emit_custom_event(
            &tx, agent_ref, actor, None, event, data, schema, &manifest,
        )?;
        crate::agent_job_events::advance_legacy_revision_without_delivery_in(
            &tx,
            agent_ref,
            outcome.event_rev(),
        )?;
        let result = project_outcome(outcome, None, schema.cloned());
        db(tx.commit())?;
        if matches!(result, EventPublishOutcome::Published(_)) {
            self.signal_event_wake();
        }
        Ok(result)
    }
}

impl SessionHandle {
    /// Lower host-runtime seam for model custom Events. The source is always this
    /// exact Agent/current branch and the actor is the immutable running Turn
    /// identity; there is no API parameter for another source.
    pub fn publish_model_event(
        &self,
        turn: u64,
        event: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<EventPublishOutcome> {
        self.publish_turn_event_inner(turn, event, data, None)
    }

    pub fn publish_typed_event<T: EventPayload>(
        &self,
        turn: u64,
        registry: &EventTypeRegistryReader,
        key: &EventKey<T>,
        value: &T,
    ) -> Result<EventPublishOutcome> {
        let descriptor = registry.revalidate(key)?;
        let data = serde_json::to_value(value)
            .map_err(|_| Error::Denied("typed Event payload cannot be encoded"))?;
        event_authority::validate_data(&data)?;
        let schema = key.schema();
        debug_assert_eq!(descriptor.event, key.event());
        self.publish_turn_event_inner(turn, key.event(), Some(&data), Some(&schema))
    }

    fn publish_turn_event_inner(
        &self,
        turn: u64,
        event: &str,
        data: Option<&serde_json::Value>,
        schema: Option<&EventSchemaRef>,
    ) -> Result<EventPublishOutcome> {
        if turn == 0 {
            return Err(Error::Denied(
                "model Event publication requires a current Turn",
            ));
        }
        let branch = self.branch()?;
        let store = self.store();
        let mut connection = store.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let agent = agent_owner::AgentKey::new(self.session_id(), &branch.id);
        let (lifecycle, _) = agent_owner::lifecycle(&tx, agent)?;
        if lifecycle != "open" {
            return Err(Error::Conflict(
                "model custom Event publication requires an open Agent",
            ));
        }
        let identity_json: Option<String> = db(tx
            .query_row(
                "SELECT r.identity_json
                 FROM runs r JOIN branches b ON b.active_run=r.id
                 WHERE r.session_id=?1 AND r.branch_id=?2 AND r.turn_id=?3
                   AND r.status='running' AND b.id=?2",
                params![self.session_id(), branch.id, turn],
                |row| row.get(0),
            )
            .optional())?;
        let Some(identity_json) = identity_json else {
            return Err(Error::Denied(
                "model Event publication requires the currently running original Turn",
            ));
        };
        let identity: InvocationIdentity = decode(&identity_json)?;
        let source_ref = agent_owner::canonical_ref(&tx, agent)?;

        let current_count: u64 = db(tx.query_row(
            "SELECT COALESCE(
                 (SELECT published_count FROM event_custom_turn_counts
                  WHERE session_id=?1 AND branch_id=?2 AND turn_id=?3),
                 0
             )",
            params![self.session_id(), branch.id, turn],
            |row| row.get(0),
        ))?;
        if current_count >= MAX_MODEL_CUSTOM_EVENTS_PER_TURN {
            return Err(Error::Limit(
                "model custom Event publications exceed 32 per Turn",
            ));
        }

        let manifest = history_owner::event_publication_manifest_for_turn(
            &tx,
            self.session_id(),
            &branch.id,
            turn,
        )?;
        let outcome = event_source::emit_custom_event(
            &tx,
            &source_ref,
            &identity.actor,
            Some(turn),
            event,
            data,
            schema,
            &manifest,
        )?;
        crate::agent_job_events::advance_legacy_revision_without_delivery_in(
            &tx,
            &source_ref,
            outcome.event_rev(),
        )?;

        let result = match outcome {
            event_source::CustomEventEmitOutcome::Published(emission) => {
                let changed = db(tx.execute(
                    "INSERT INTO event_custom_turn_counts(
                         session_id,branch_id,turn_id,published_count,updated_at
                     ) VALUES(?1,?2,?3,1,?4)
                     ON CONFLICT(session_id,branch_id,turn_id) DO UPDATE SET
                         published_count=event_custom_turn_counts.published_count+1,
                         updated_at=excluded.updated_at
                     WHERE event_custom_turn_counts.published_count<32",
                    params![self.session_id(), branch.id, turn, now()],
                ))?;
                if changed != 1 {
                    return Err(Error::Limit(
                        "model custom Event publications exceed 32 per Turn",
                    ));
                }
                EventPublishOutcome::Published(EventPublished {
                    subject: emission.subject,
                    event_rev: emission.event_rev,
                    turn: Some(turn),
                    causal_root: emission.causal_root,
                    causal_hop: emission.causal_hop,
                    schema: schema.cloned(),
                })
            }
            event_source::CustomEventEmitOutcome::CausalExhausted { event_rev, root } => {
                EventPublishOutcome::CausalExhausted {
                    event_rev,
                    causal_root: root,
                }
            }
        };
        db(tx.commit())?;
        if matches!(result, EventPublishOutcome::Published(_)) {
            store.signal_event_wake();
        }
        Ok(result)
    }
}

fn project_outcome(
    outcome: event_source::CustomEventEmitOutcome,
    turn: Option<u64>,
    schema: Option<EventSchemaRef>,
) -> EventPublishOutcome {
    match outcome {
        event_source::CustomEventEmitOutcome::Published(emission) => {
            EventPublishOutcome::Published(EventPublished {
                subject: emission.subject,
                event_rev: emission.event_rev,
                turn,
                causal_root: emission.causal_root,
                causal_hop: emission.causal_hop,
                schema,
            })
        }
        event_source::CustomEventEmitOutcome::CausalExhausted { event_rev, root } => {
            EventPublishOutcome::CausalExhausted {
                event_rev,
                causal_root: root,
            }
        }
    }
}
