//! Durable DL-200 EventHandler activation, receipt and ACK owner.
//!
//! Handler definitions live in EventHandlerRegistry. This module persists only
//! activation/progress plus one ordered in-flight receipt. Handler/product state
//! and external-effect idempotency stay with the handler host.

use crate::{
    agent_owner, db, event_authority,
    event_placement::{self, EventPlacementSnapshot, EventSelectorPlacement},
    event_read::{self, EventReadEvent},
    event_retention,
    event_scan::{self, EventAuthorization, EventScanFence, EventScanResult, MAX_EVENT_SCAN_ROWS},
    now,
    store_scope::{StorePos, StoreScope},
    Error, PrincipalId, Result, SessionStore,
};
use aw_agent_core::EventHandlerDescriptor;
use aw_tools::event_subject::EventPattern;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventHandlerStart {
    Now,
    EarliestAvailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventHandlerState {
    Active,
    Suspended,
    Lagged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventHandlerAuthority {
    pub principal: PrincipalId,
    pub reference: String,
    pub generation: u64,
}

impl EventHandlerAuthority {
    pub fn new(
        principal: PrincipalId,
        reference: impl Into<String>,
        generation: u64,
    ) -> Result<Self> {
        let reference = reference.into();
        if reference.is_empty()
            || reference.len() > 256
            || !reference.is_ascii()
            || reference.chars().any(char::is_control)
            || generation == 0
        {
            return Err(Error::Denied("invalid EventHandler service authority"));
        }
        Ok(Self {
            principal,
            reference,
            generation,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHandlerActivation {
    pub extension: String,
    pub handler: String,
    pub activation_generation: u64,
    pub state: EventHandlerState,
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHandlerDelivery {
    pub receipt: String,
    pub event: EventReadEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "delivery")]
pub enum EventHandlerPoll {
    Delivery(Box<EventHandlerDelivery>),
    Empty,
    Blocked,
    Suspended,
    Lagged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHandlerAck {
    pub receipt: String,
    pub advanced: bool,
}

#[derive(Clone, Debug)]
struct StoredHandler {
    extension: String,
    handler: String,
    schema_version: u32,
    registration_revision: u64,
    pattern: String,
    selector_sha256: [u8; 32],
    activation_generation: u64,
    state: EventHandlerState,
    service_principal: String,
    authority_ref: String,
    authority_generation: u64,
    store_id: String,
    restore_generation: u64,
    placement_kind: String,
    placement_ref: String,
    placement_token: [u8; 32],
    scan_position: u64,
    last_acked_receipt: Option<String>,
}

#[derive(Clone, Debug)]
struct StoredReceipt {
    activation_generation: u64,
    receipt_id: String,
    event_id: String,
    event_position: u64,
}

impl SessionStore {
    pub fn events_handler_activate(
        &self,
        definition: &EventHandlerDescriptor,
        authority: &EventHandlerAuthority,
        start: EventHandlerStart,
    ) -> Result<EventHandlerActivation> {
        let (pattern, placement) = validate_definition(definition)?;
        validate_authority(authority)?;

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let _placement_snapshot = event_placement::admit(&tx, &authority.principal, &placement)?;
        let projection = crate::event_projection::snapshot(&tx)?;
        if !projection.usable {
            return Err(Error::Storage(
                "Event projection is unavailable; owner normalization required",
            ));
        }
        let selector_sha256 = event_scan::selector_hash(pattern.as_str());
        let placement_token = placement.token(&projection.scope);
        let current = handler_row(&tx, &definition.extension, &definition.id)?;

        if let Some(current) = current.as_ref() {
            if same_logical_activation(current, definition, authority, &placement, &selector_sha256)
            {
                if current.state == EventHandlerState::Lagged {
                    return Ok(handler_activation(current, false));
                }
                if current.store_id != projection.scope.id().as_str()
                    || current.restore_generation != projection.scope.restore_generation()
                    || current.placement_token != placement_token
                {
                    lag_handler(&tx, current, "store_scope_stale")?;
                    db(tx.commit())?;
                    return Ok(EventHandlerActivation {
                        extension: current.extension.clone(),
                        handler: current.handler.clone(),
                        activation_generation: current.activation_generation,
                        state: EventHandlerState::Lagged,
                        changed: true,
                    });
                }
            } else if current.state == EventHandlerState::Lagged {
                return Err(Error::Conflict(
                    "lagged EventHandler requires explicit rebaseline",
                ));
            }
            if same_compatible_activation(
                current,
                definition,
                authority,
                &placement,
                &projection.scope,
                &selector_sha256,
                &placement_token,
            ) {
                if event_retention::expired_now(&tx, StorePos::new(current.scan_position))? {
                    lag_handler(&tx, current, "retention_overtook_scan")?;
                    db(tx.commit())?;
                    return Ok(EventHandlerActivation {
                        extension: current.extension.clone(),
                        handler: current.handler.clone(),
                        activation_generation: current.activation_generation,
                        state: EventHandlerState::Lagged,
                        changed: true,
                    });
                }
                let changed = current.state != EventHandlerState::Active
                    || current.registration_revision != definition.registration_revision;
                if changed {
                    let updated = db(tx.execute(
                        "UPDATE event_handler_cursors_current
                         SET registration_revision=?1,state='active',lag_reason=NULL,updated_at=?2
                         WHERE extension_id=?3 AND handler_id=?4
                           AND activation_generation=?5",
                        params![
                            definition.registration_revision,
                            now(),
                            definition.extension,
                            definition.id,
                            current.activation_generation,
                        ],
                    ))?;
                    if updated != 1 {
                        return Err(Error::Conflict(
                            "EventHandler activation changed concurrently",
                        ));
                    }
                }
                db(tx.commit())?;
                return Ok(EventHandlerActivation {
                    extension: definition.extension.clone(),
                    handler: definition.id.clone(),
                    activation_generation: current.activation_generation,
                    state: EventHandlerState::Active,
                    changed,
                });
            }
        }

        let baseline = handler_baseline(&tx, start, projection.indexed_through)?;
        let generation = current
            .as_ref()
            .map(|current| {
                current
                    .activation_generation
                    .checked_add(1)
                    .ok_or(Error::Limit("EventHandler activation generation exhausted"))
            })
            .transpose()?
            .unwrap_or(1);
        let at = now();
        if current.is_some() {
            db(tx.execute(
                "DELETE FROM event_handler_receipts_current
                 WHERE extension_id=?1 AND handler_id=?2",
                params![definition.extension, definition.id],
            ))?;
            let changed = db(tx.execute(
                "UPDATE event_handler_cursors_current SET
                     handler_schema_version=?1,registration_revision=?2,pattern=?3,
                     selector_sha256=?4,activation_generation=?5,state='active',
                     service_principal=?6,authority_ref=?7,authority_generation=?8,
                     store_id=?9,restore_generation=?10,placement_kind=?11,
                     placement_ref=?12,placement_token=?13,baseline_position=?14,
                     scan_position=?14,last_acked_receipt_id=NULL,lag_reason=NULL,updated_at=?15
                 WHERE extension_id=?16 AND handler_id=?17",
                params![
                    definition.schema_version,
                    definition.registration_revision,
                    pattern.as_str(),
                    selector_sha256.as_slice(),
                    generation,
                    authority.principal.as_str(),
                    authority.reference,
                    authority.generation,
                    projection.scope.id().as_str(),
                    projection.scope.restore_generation(),
                    placement.kind(),
                    placement.reference(),
                    placement_token.as_slice(),
                    baseline.get(),
                    at,
                    definition.extension,
                    definition.id,
                ],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "EventHandler activation changed concurrently",
                ));
            }
        } else {
            db(tx.execute(
                "INSERT INTO event_handler_cursors_current(
                     extension_id,handler_id,handler_schema_version,registration_revision,
                     pattern,selector_sha256,activation_generation,state,service_principal,
                     authority_ref,authority_generation,store_id,restore_generation,
                     placement_kind,placement_ref,placement_token,baseline_position,
                     scan_position,last_acked_receipt_id,lag_reason,created_at,updated_at
                 ) VALUES(
                     ?1,?2,?3,?4,?5,?6,1,'active',?7,?8,?9,?10,?11,
                     ?12,?13,?14,?15,?15,NULL,NULL,?16,?16
                 )",
                params![
                    definition.extension,
                    definition.id,
                    definition.schema_version,
                    definition.registration_revision,
                    pattern.as_str(),
                    selector_sha256.as_slice(),
                    authority.principal.as_str(),
                    authority.reference,
                    authority.generation,
                    projection.scope.id().as_str(),
                    projection.scope.restore_generation(),
                    placement.kind(),
                    placement.reference(),
                    placement_token.as_slice(),
                    baseline.get(),
                    at,
                ],
            ))?;
        }
        db(tx.commit())?;
        Ok(EventHandlerActivation {
            extension: definition.extension.clone(),
            handler: definition.id.clone(),
            activation_generation: generation,
            state: EventHandlerState::Active,
            changed: true,
        })
    }

    pub fn events_handler_rebaseline(
        &self,
        definition: &EventHandlerDescriptor,
        authority: &EventHandlerAuthority,
        start: EventHandlerStart,
    ) -> Result<EventHandlerActivation> {
        let (pattern, placement) = validate_definition(definition)?;
        validate_authority(authority)?;

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = handler_row(&tx, &definition.extension, &definition.id)?
            .ok_or(Error::Denied("EventHandler activation is unavailable"))?;
        let _placement_snapshot = event_placement::admit(&tx, &authority.principal, &placement)?;
        let projection = crate::event_projection::snapshot(&tx)?;
        if !projection.usable {
            return Err(Error::Storage(
                "Event projection is unavailable; owner normalization required",
            ));
        }
        let baseline = handler_baseline(&tx, start, projection.indexed_through)?;
        let generation = current
            .activation_generation
            .checked_add(1)
            .ok_or(Error::Limit("EventHandler activation generation exhausted"))?;
        let selector_sha256 = event_scan::selector_hash(pattern.as_str());
        let placement_token = placement.token(&projection.scope);
        db(tx.execute(
            "DELETE FROM event_handler_receipts_current
             WHERE extension_id=?1 AND handler_id=?2",
            params![definition.extension, definition.id],
        ))?;
        let changed = db(tx.execute(
            "UPDATE event_handler_cursors_current SET
                 handler_schema_version=?1,registration_revision=?2,pattern=?3,
                 selector_sha256=?4,activation_generation=?5,state='active',
                 service_principal=?6,authority_ref=?7,authority_generation=?8,
                 store_id=?9,restore_generation=?10,placement_kind=?11,
                 placement_ref=?12,placement_token=?13,baseline_position=?14,
                 scan_position=?14,last_acked_receipt_id=NULL,lag_reason=NULL,updated_at=?15
             WHERE extension_id=?16 AND handler_id=?17
               AND activation_generation=?18",
            params![
                definition.schema_version,
                definition.registration_revision,
                pattern.as_str(),
                selector_sha256.as_slice(),
                generation,
                authority.principal.as_str(),
                authority.reference,
                authority.generation,
                projection.scope.id().as_str(),
                projection.scope.restore_generation(),
                placement.kind(),
                placement.reference(),
                placement_token.as_slice(),
                baseline.get(),
                now(),
                definition.extension,
                definition.id,
                current.activation_generation,
            ],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "EventHandler rebaseline changed concurrently",
            ));
        }
        db(tx.commit())?;
        Ok(EventHandlerActivation {
            extension: definition.extension.clone(),
            handler: definition.id.clone(),
            activation_generation: generation,
            state: EventHandlerState::Active,
            changed: true,
        })
    }

    pub fn events_handler_suspend(
        &self,
        extension: &str,
        handler: &str,
        authority: &EventHandlerAuthority,
    ) -> Result<EventHandlerActivation> {
        validate_authority(authority)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = handler_row(&tx, extension, handler)?
            .ok_or(Error::Denied("EventHandler activation is unavailable"))?;
        require_authority(&current, authority)?;
        if current.state == EventHandlerState::Suspended {
            db(tx.commit())?;
            return Ok(handler_activation(&current, false));
        }
        let changed = db(tx.execute(
            "UPDATE event_handler_cursors_current
             SET state='suspended',lag_reason=NULL,updated_at=?1
             WHERE extension_id=?2 AND handler_id=?3 AND activation_generation=?4",
            params![now(), extension, handler, current.activation_generation],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict("EventHandler suspend changed concurrently"));
        }
        db(tx.commit())?;
        Ok(EventHandlerActivation {
            extension: extension.to_owned(),
            handler: handler.to_owned(),
            activation_generation: current.activation_generation,
            state: EventHandlerState::Suspended,
            changed: true,
        })
    }

    pub fn events_handler_poll(
        &self,
        definition: &EventHandlerDescriptor,
        authority: &EventHandlerAuthority,
    ) -> Result<EventHandlerPoll> {
        let (pattern, _declared_placement) = validate_definition(definition)?;
        validate_authority(authority)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = handler_row(&tx, &definition.extension, &definition.id)?
            .ok_or(Error::Denied("EventHandler activation is unavailable"))?;
        require_definition(&current, definition)?;
        require_authority(&current, authority)?;
        match current.state {
            EventHandlerState::Suspended => {
                db(tx.commit())?;
                return Ok(EventHandlerPoll::Suspended);
            }
            EventHandlerState::Lagged => {
                db(tx.commit())?;
                return Ok(EventHandlerPoll::Lagged);
            }
            EventHandlerState::Active => {}
        }

        let placement = match validate_current_placement(&tx, &current, authority) {
            Ok(placement) => placement,
            Err(Error::Conflict(_)) => {
                lag_handler(&tx, &current, "store_scope_stale")?;
                db(tx.commit())?;
                return Ok(EventHandlerPoll::Lagged);
            }
            Err(other) => return Err(other),
        };
        let placement_snapshot = match handler_placement_check(&tx, &placement, authority)? {
            HandlerPlacementCheck::Current(snapshot) => snapshot,
            HandlerPlacementCheck::Blocked => {
                db(tx.commit())?;
                return Ok(EventHandlerPoll::Blocked);
            }
            HandlerPlacementCheck::Stale => {
                lag_handler(&tx, &current, "placement_stale")?;
                db(tx.commit())?;
                return Ok(EventHandlerPoll::Lagged);
            }
        };
        if event_retention::expired_now(&tx, StorePos::new(current.scan_position))? {
            lag_handler(&tx, &current, "retention_overtook_scan")?;
            db(tx.commit())?;
            return Ok(EventHandlerPoll::Lagged);
        }

        let receipt = receipt_row(&tx, &current.extension, &current.handler)?;
        let result = scan_handler_once(
            &tx,
            &current,
            authority,
            &pattern,
            &placement,
            &placement_snapshot,
        )?;
        let poll = if let Some(receipt) = receipt {
            match result {
                EventScanResult::Candidate { candidate, .. }
                    if candidate.event.event_id == receipt.event_id
                        && candidate.event.position.get() == receipt.event_position
                        && receipt.activation_generation == current.activation_generation =>
                {
                    EventHandlerPoll::Delivery(Box::new(EventHandlerDelivery {
                        receipt: receipt.receipt_id,
                        event: event_read::project_event(
                            &candidate.event,
                            candidate.candidate_evidence,
                        ),
                    }))
                }
                EventScanResult::Stale => {
                    lag_handler(&tx, &current, "placement_stale")?;
                    EventHandlerPoll::Lagged
                }
                _ => EventHandlerPoll::Blocked,
            }
        } else {
            match result {
                EventScanResult::Candidate { candidate, .. } => {
                    let receipt_id = handler_receipt_id(
                        &current.extension,
                        &current.handler,
                        current.activation_generation,
                        &candidate.event.event_id,
                    );
                    db(tx.execute(
                        "INSERT INTO event_handler_receipts_current(
                             extension_id,handler_id,activation_generation,receipt_id,
                             event_id,event_position,created_at
                         ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        params![
                            current.extension,
                            current.handler,
                            current.activation_generation,
                            receipt_id,
                            candidate.event.event_id,
                            candidate.event.position.get(),
                            now(),
                        ],
                    ))?;
                    EventHandlerPoll::Delivery(Box::new(EventHandlerDelivery {
                        receipt: receipt_id,
                        event: event_read::project_event(
                            &candidate.event,
                            candidate.candidate_evidence,
                        ),
                    }))
                }
                EventScanResult::ProvenSkip { through, .. } | EventScanResult::End { through } => {
                    advance_handler_scan(&tx, &current, through)?;
                    EventHandlerPoll::Empty
                }
                EventScanResult::BlockedUnknown => EventHandlerPoll::Blocked,
                EventScanResult::Stale => {
                    lag_handler(&tx, &current, "placement_stale")?;
                    EventHandlerPoll::Lagged
                }
            }
        };
        db(tx.commit())?;
        Ok(poll)
    }

    pub fn events_handler_ack(
        &self,
        definition: &EventHandlerDescriptor,
        authority: &EventHandlerAuthority,
        receipt_id: &str,
    ) -> Result<EventHandlerAck> {
        let (pattern, _declared_placement) = validate_definition(definition)?;
        validate_authority(authority)?;
        validate_receipt_id(receipt_id)?;

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let current = handler_row(&tx, &definition.extension, &definition.id)?
            .ok_or(Error::Denied("EventHandler activation is unavailable"))?;
        require_definition(&current, definition)?;
        require_authority(&current, authority)?;
        if current.last_acked_receipt.as_deref() == Some(receipt_id) {
            db(tx.commit())?;
            return Ok(EventHandlerAck {
                receipt: receipt_id.to_owned(),
                advanced: false,
            });
        }
        if current.state != EventHandlerState::Active {
            return Err(Error::Denied("EventHandler is not active"));
        }
        let receipt = receipt_row(&tx, &current.extension, &current.handler)?
            .filter(|receipt| receipt.receipt_id == receipt_id)
            .ok_or(Error::Denied("EventHandler receipt is unavailable"))?;
        if receipt.activation_generation != current.activation_generation {
            return Err(Error::Denied("EventHandler receipt activation is stale"));
        }

        let placement = match validate_current_placement(&tx, &current, authority) {
            Ok(placement) => placement,
            Err(Error::Conflict(_)) => {
                lag_handler(&tx, &current, "store_scope_stale")?;
                db(tx.commit())?;
                return Err(Error::Conflict("EventHandler became stale before ACK"));
            }
            Err(other) => return Err(other),
        };
        let placement_snapshot = match handler_placement_check(&tx, &placement, authority)? {
            HandlerPlacementCheck::Current(snapshot) => snapshot,
            HandlerPlacementCheck::Blocked => {
                return Err(Error::Conflict(
                    "EventHandler authority unavailable before ACK",
                ))
            }
            HandlerPlacementCheck::Stale => {
                lag_handler(&tx, &current, "placement_stale")?;
                db(tx.commit())?;
                return Err(Error::Conflict("EventHandler became stale before ACK"));
            }
        };
        match scan_handler_once(
            &tx,
            &current,
            authority,
            &pattern,
            &placement,
            &placement_snapshot,
        )? {
            EventScanResult::Candidate { candidate, .. }
                if candidate.event.event_id == receipt.event_id
                    && candidate.event.position.get() == receipt.event_position =>
            {
                db(tx.execute(
                    "DELETE FROM event_handler_receipts_current
                     WHERE extension_id=?1 AND handler_id=?2 AND receipt_id=?3",
                    params![current.extension, current.handler, receipt_id],
                ))?;
                let changed = db(tx.execute(
                    "UPDATE event_handler_cursors_current
                     SET scan_position=?1,last_acked_receipt_id=?2,updated_at=?3
                     WHERE extension_id=?4 AND handler_id=?5
                       AND activation_generation=?6 AND state='active'
                       AND scan_position=?7",
                    params![
                        receipt.event_position,
                        receipt_id,
                        now(),
                        current.extension,
                        current.handler,
                        current.activation_generation,
                        current.scan_position,
                    ],
                ))?;
                if changed != 1 {
                    return Err(Error::Conflict("EventHandler ACK changed concurrently"));
                }
            }
            EventScanResult::Stale => {
                lag_handler(&tx, &current, "placement_stale")?;
                db(tx.commit())?;
                return Err(Error::Conflict("EventHandler became stale before ACK"));
            }
            _ => {
                return Err(Error::Conflict(
                    "EventHandler receipt cannot be reauthorized before ACK",
                ))
            }
        }
        db(tx.commit())?;
        Ok(EventHandlerAck {
            receipt: receipt_id.to_owned(),
            advanced: true,
        })
    }
}

fn validate_definition(
    definition: &EventHandlerDescriptor,
) -> Result<(EventPattern, EventSelectorPlacement)> {
    if definition.extension.is_empty()
        || definition.extension.len() > 48
        || definition.id.is_empty()
        || definition.id.len() > 48
        || definition.schema_version == 0
        || definition.registration_revision == 0
    {
        return Err(Error::Denied("invalid EventHandler definition identity"));
    }
    let pattern = EventPattern::parse_absolute(&definition.pattern)
        .map_err(|_| Error::Denied("invalid EventHandler pattern"))?;
    let placement = EventSelectorPlacement::from_pattern(&pattern)?;
    Ok((pattern, placement))
}

fn validate_authority(authority: &EventHandlerAuthority) -> Result<()> {
    if authority.reference.is_empty()
        || authority.reference.len() > 256
        || !authority.reference.is_ascii()
        || authority.reference.chars().any(char::is_control)
        || authority.generation == 0
    {
        return Err(Error::Denied("invalid EventHandler service authority"));
    }
    Ok(())
}

fn validate_receipt_id(receipt: &str) -> Result<()> {
    if receipt.len() != 68
        || !receipt.starts_with("ehr_")
        || !receipt[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::Denied("invalid EventHandler receipt identity"));
    }
    Ok(())
}

fn handler_baseline(
    connection: &Connection,
    start: EventHandlerStart,
    now_tail: StorePos,
) -> Result<StorePos> {
    match start {
        EventHandlerStart::Now => Ok(now_tail),
        EventHandlerStart::EarliestAvailable => {
            event_retention::earliest_available_after(connection)
        }
    }
}

fn same_logical_activation(
    current: &StoredHandler,
    definition: &EventHandlerDescriptor,
    authority: &EventHandlerAuthority,
    placement: &EventSelectorPlacement,
    selector_sha256: &[u8; 32],
) -> bool {
    current.schema_version == definition.schema_version
        && current.pattern == definition.pattern
        && current.selector_sha256 == *selector_sha256
        && current.service_principal == authority.principal.as_str()
        && current.authority_ref == authority.reference
        && current.authority_generation == authority.generation
        && current.placement_kind == placement.kind()
        && current.placement_ref == placement.reference()
}

fn same_compatible_activation(
    current: &StoredHandler,
    definition: &EventHandlerDescriptor,
    authority: &EventHandlerAuthority,
    placement: &EventSelectorPlacement,
    scope: &StoreScope,
    selector_sha256: &[u8; 32],
    placement_token: &[u8; 32],
) -> bool {
    same_logical_activation(current, definition, authority, placement, selector_sha256)
        && current.store_id == scope.id().as_str()
        && current.restore_generation == scope.restore_generation()
        && current.placement_token == *placement_token
}

fn require_definition(current: &StoredHandler, definition: &EventHandlerDescriptor) -> Result<()> {
    if current.schema_version != definition.schema_version
        || current.registration_revision != definition.registration_revision
        || current.pattern != definition.pattern
    {
        return Err(Error::Conflict(
            "EventHandler definition changed; explicit activation is required",
        ));
    }
    Ok(())
}

fn require_authority(current: &StoredHandler, authority: &EventHandlerAuthority) -> Result<()> {
    if current.service_principal != authority.principal.as_str()
        || current.authority_ref != authority.reference
        || current.authority_generation != authority.generation
    {
        return Err(Error::Denied("EventHandler service authority changed"));
    }
    Ok(())
}

fn validate_current_placement(
    connection: &Connection,
    current: &StoredHandler,
    authority: &EventHandlerAuthority,
) -> Result<EventSelectorPlacement> {
    let scope = StoreScope::load(connection)?;
    let placement = EventSelectorPlacement::from_stored(
        &current.placement_kind,
        current.placement_ref.clone(),
    )?;
    if current.store_id != scope.id().as_str()
        || current.restore_generation != scope.restore_generation()
        || current.placement_token != placement.token(&scope)
    {
        return Err(Error::Conflict(
            "EventHandler StoreScope/placement is stale",
        ));
    }
    let parsed = EventPattern::parse_absolute(&current.pattern)
        .map_err(|_| Error::Storage("stored EventHandler pattern is invalid"))?;
    if EventSelectorPlacement::from_pattern(&parsed)? != placement {
        return Err(Error::Storage(
            "stored EventHandler pattern disagrees with placement",
        ));
    }
    require_authority(current, authority)?;
    Ok(placement)
}

enum HandlerPlacementCheck {
    Current(EventPlacementSnapshot),
    Blocked,
    Stale,
}

fn handler_placement_check(
    connection: &Connection,
    placement: &EventSelectorPlacement,
    authority: &EventHandlerAuthority,
) -> Result<HandlerPlacementCheck> {
    let structural = match event_placement::snapshot(connection, placement) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return Ok(HandlerPlacementCheck::Stale),
        Err(Error::Conflict(_)) => return Ok(HandlerPlacementCheck::Blocked),
        Err(other) => return Err(other),
    };
    match event_placement::admit(connection, &authority.principal, placement) {
        Ok(admitted) if admitted == structural => Ok(HandlerPlacementCheck::Current(structural)),
        Ok(_) => Err(Error::Storage(
            "EventHandler placement authority disagrees with structural placement",
        )),
        Err(Error::Denied(_)) | Err(Error::Conflict(_)) => Ok(HandlerPlacementCheck::Blocked),
        Err(other) => Err(other),
    }
}

fn scan_handler_once(
    connection: &Connection,
    current: &StoredHandler,
    authority: &EventHandlerAuthority,
    pattern: &EventPattern,
    placement: &EventSelectorPlacement,
    placement_snapshot: &EventPlacementSnapshot,
) -> Result<EventScanResult<event_authority::EventSourceEvidence, event_authority::EventEnvelope>> {
    let fence = EventScanFence::capture(connection)?;
    let mut authority_cache =
        HashMap::<String, EventAuthorization<agent_owner::AgentAccessEvidence>>::new();
    event_scan::scan_events(
        connection,
        &fence,
        StorePos::new(current.scan_position),
        pattern,
        MAX_EVENT_SCAN_ROWS,
        || event_placement::state(connection, placement, placement_snapshot),
        |source| {
            event_authority::source_visibility(
                connection,
                &authority.principal,
                placement,
                source,
                &mut authority_cache,
            )
        },
        |candidate, _source_evidence| {
            event_authority::candidate_authority(connection, &authority.principal, candidate)
        },
    )
}

fn advance_handler_scan(
    connection: &Connection,
    current: &StoredHandler,
    through: StorePos,
) -> Result<()> {
    if through.get() == current.scan_position {
        return Ok(());
    }
    let changed = db(connection.execute(
        "UPDATE event_handler_cursors_current
         SET scan_position=?1,updated_at=?2
         WHERE extension_id=?3 AND handler_id=?4
           AND activation_generation=?5 AND state='active'
           AND scan_position=?6",
        params![
            through.get(),
            now(),
            current.extension,
            current.handler,
            current.activation_generation,
            current.scan_position,
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "EventHandler scan progress changed concurrently",
        ));
    }
    Ok(())
}

fn lag_handler(connection: &Connection, current: &StoredHandler, reason: &str) -> Result<()> {
    db(connection.execute(
        "DELETE FROM event_handler_receipts_current
         WHERE extension_id=?1 AND handler_id=?2",
        params![current.extension, current.handler],
    ))?;
    let changed = db(connection.execute(
        "UPDATE event_handler_cursors_current
         SET state='lagged',lag_reason=?1,updated_at=?2
         WHERE extension_id=?3 AND handler_id=?4
           AND activation_generation=?5 AND state IN ('active','suspended')",
        params![
            reason,
            now(),
            current.extension,
            current.handler,
            current.activation_generation,
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "EventHandler lag state changed concurrently",
        ));
    }
    Ok(())
}

fn handler_activation(current: &StoredHandler, changed: bool) -> EventHandlerActivation {
    EventHandlerActivation {
        extension: current.extension.clone(),
        handler: current.handler.clone(),
        activation_generation: current.activation_generation,
        state: current.state,
        changed,
    }
}

fn handler_receipt_id(
    extension: &str,
    handler: &str,
    activation_generation: u64,
    event_id: &str,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-event-handler-receipt-v1\0");
    for part in [
        extension.as_bytes(),
        handler.as_bytes(),
        event_id.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.update(activation_generation.to_be_bytes());
    format!("ehr_{:x}", hash.finalize())
}

fn handler_row(
    connection: &Connection,
    extension: &str,
    handler: &str,
) -> Result<Option<StoredHandler>> {
    db(connection
        .query_row(
            "SELECT extension_id,handler_id,handler_schema_version,registration_revision,
                    pattern,selector_sha256,activation_generation,state,service_principal,
                    authority_ref,authority_generation,store_id,restore_generation,
                    placement_kind,placement_ref,placement_token,
                    scan_position,last_acked_receipt_id
             FROM event_handler_cursors_current
             WHERE extension_id=?1 AND handler_id=?2",
            params![extension, handler],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, u64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, u64>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, Vec<u8>>(15)?,
                    row.get::<_, u64>(16)?,
                    row.get::<_, Option<String>>(17)?,
                ))
            },
        )
        .optional())?
    .map(
        |(
            extension,
            handler,
            schema_version,
            registration_revision,
            pattern,
            selector_sha256,
            activation_generation,
            state,
            service_principal,
            authority_ref,
            authority_generation,
            store_id,
            restore_generation,
            placement_kind,
            placement_ref,
            placement_token,
            scan_position,
            last_acked_receipt,
        )| {
            let selector_sha256: [u8; 32] = selector_sha256
                .try_into()
                .map_err(|_| Error::Storage("invalid EventHandler selector hash"))?;
            let placement_token: [u8; 32] = placement_token
                .try_into()
                .map_err(|_| Error::Storage("invalid EventHandler placement token"))?;
            let state = match state.as_str() {
                "active" => EventHandlerState::Active,
                "suspended" => EventHandlerState::Suspended,
                "lagged" => EventHandlerState::Lagged,
                _ => return Err(Error::Storage("invalid EventHandler state")),
            };
            Ok(StoredHandler {
                extension,
                handler,
                schema_version,
                registration_revision,
                pattern,
                selector_sha256,
                activation_generation,
                state,
                service_principal,
                authority_ref,
                authority_generation,
                store_id,
                restore_generation,
                placement_kind,
                placement_ref,
                placement_token,
                scan_position,
                last_acked_receipt,
            })
        },
    )
    .transpose()
}

fn receipt_row(
    connection: &Connection,
    extension: &str,
    handler: &str,
) -> Result<Option<StoredReceipt>> {
    db(connection
        .query_row(
            "SELECT activation_generation,receipt_id,event_id,event_position
             FROM event_handler_receipts_current
             WHERE extension_id=?1 AND handler_id=?2",
            params![extension, handler],
            |row| {
                Ok(StoredReceipt {
                    activation_generation: row.get(0)?,
                    receipt_id: row.get(1)?,
                    event_id: row.get(2)?,
                    event_position: row.get(3)?,
                })
            },
        )
        .optional())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionBinding;
    use aw_agent_core::EventHandlerRegistry;
    use aw_store_api::SessionBackend as _;
    use std::path::PathBuf;

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

    #[test]
    fn handler_receipt_replays_until_ack_and_suspend_preserves_backlog() {
        let store = SessionStore::in_memory().unwrap();
        let actor = PrincipalId::new("internal:fixture:event-handler").unwrap();
        let (session, branch) = store.create(&actor, "handler source", binding()).unwrap();
        let binding = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let agent_ref = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            binding.domain_handle,
            binding.identity_handle,
            binding.project_handle,
            binding.agent_handle
        );
        let subject =
            aw_tools::event_subject::EventSubject::from_resource_ref(&agent_ref, "#ready").unwrap();

        let registry = EventHandlerRegistry::new();
        let key = registry
            .register("fixture", "indexer", subject.as_str(), 1, ())
            .unwrap();
        let reader = registry.reader().unwrap();
        let definition = reader.descriptor(&key).unwrap();
        let authority =
            EventHandlerAuthority::new(actor.clone(), "svc:fixture:indexer", 1).unwrap();

        store
            .events_publish(
                &actor,
                &agent_ref,
                "#ready",
                Some(&serde_json::json!({"n":1})),
            )
            .unwrap();

        let active = store
            .events_handler_activate(
                &definition,
                &authority,
                EventHandlerStart::EarliestAvailable,
            )
            .unwrap();
        assert_eq!(active.state, EventHandlerState::Active);
        assert_eq!(active.activation_generation, 1);

        let first = match store.events_handler_poll(&definition, &authority).unwrap() {
            EventHandlerPoll::Delivery(delivery) => delivery,
            other => panic!("unexpected first Handler poll: {other:?}"),
        };
        assert_eq!(first.event.subject, subject.as_str());
        assert_eq!(first.event.data, Some(serde_json::json!({"n":1})));

        let replay = match store.events_handler_poll(&definition, &authority).unwrap() {
            EventHandlerPoll::Delivery(delivery) => delivery,
            other => panic!("unexpected Handler receipt replay: {other:?}"),
        };
        assert_eq!(replay.receipt, first.receipt);
        assert_eq!(replay.event.id, first.event.id);

        let ack = store
            .events_handler_ack(&definition, &authority, &first.receipt)
            .unwrap();
        assert!(ack.advanced);
        let replayed_ack = store
            .events_handler_ack(&definition, &authority, &first.receipt)
            .unwrap();
        assert!(!replayed_ack.advanced);
        assert_eq!(
            store.events_handler_poll(&definition, &authority).unwrap(),
            EventHandlerPoll::Empty
        );

        store
            .events_publish(
                &actor,
                &agent_ref,
                "#ready",
                Some(&serde_json::json!({"n":2})),
            )
            .unwrap();
        let second = match store.events_handler_poll(&definition, &authority).unwrap() {
            EventHandlerPoll::Delivery(delivery) => delivery,
            other => panic!("unexpected second Handler delivery: {other:?}"),
        };
        assert_ne!(second.receipt, first.receipt);

        let suspended = store
            .events_handler_suspend("fixture", "indexer", &authority)
            .unwrap();
        assert_eq!(suspended.state, EventHandlerState::Suspended);
        assert_eq!(
            store.events_handler_poll(&definition, &authority).unwrap(),
            EventHandlerPoll::Suspended
        );

        let resumed = store
            .events_handler_activate(&definition, &authority, EventHandlerStart::Now)
            .unwrap();
        assert!(resumed.changed);
        assert_eq!(resumed.activation_generation, 1);
        let replay_after_resume = match store.events_handler_poll(&definition, &authority).unwrap()
        {
            EventHandlerPoll::Delivery(delivery) => delivery,
            other => panic!("unexpected resumed Handler receipt: {other:?}"),
        };
        assert_eq!(replay_after_resume.receipt, second.receipt);
        assert_eq!(
            replay_after_resume.event.data,
            Some(serde_json::json!({"n":2}))
        );
        store
            .events_handler_ack(&definition, &authority, &second.receipt)
            .unwrap();
        assert_eq!(
            store.events_handler_poll(&definition, &authority).unwrap(),
            EventHandlerPoll::Empty
        );

        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "UPDATE event_handler_cursors_current
                     SET state='lagged',lag_reason='fixture'
                     WHERE extension_id='fixture' AND handler_id='indexer'",
                    [],
                )
                .unwrap();
        }
        let still_lagged = store
            .events_handler_activate(&definition, &authority, EventHandlerStart::Now)
            .unwrap();
        assert_eq!(still_lagged.state, EventHandlerState::Lagged);
        assert!(!still_lagged.changed);
        assert_eq!(still_lagged.activation_generation, 1);

        let rebaselined = store
            .events_handler_rebaseline(&definition, &authority, EventHandlerStart::Now)
            .unwrap();
        assert_eq!(rebaselined.state, EventHandlerState::Active);
        assert_eq!(rebaselined.activation_generation, 2);
        assert!(rebaselined.changed);
    }
}
