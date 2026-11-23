//! Durable DL-200 Event subscription state owner.
//!
//! This module owns subscription identity, generation, baseline/private scan
//! progress and lifecycle fencing. It does not copy Event backlog, admit
//! EventInputs, run delivery workers or acknowledge EventHandlers.

use crate::{
    agent_owner, db, event_authority, event_input,
    event_placement::{self, EventSelectorPlacement},
    event_projection, event_retention,
    event_scan::{self, EventAuthorization, EventScanFence, EventScanResult, MAX_EVENT_SCAN_ROWS},
    now,
    store_scope::{StorePos, StoreScope},
    Error, PrincipalId, Result, SessionStore,
};
use aw_tools::event_subject::EventPattern;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const HOST_AUTHORITY_REF: &str = "host:events.subscribe";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSubscriptionState {
    Active,
    Lagged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSubscriptionUpdate {
    pub recipient_ref: String,
    pub subject: String,
    pub enabled: bool,
    pub changed: bool,
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EventSubscriptionState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSubscriptionView {
    pub recipient_ref: String,
    pub subject: String,
    pub generation: u64,
    pub state: EventSubscriptionState,
    pub created_by: String,
    pub authority_ref: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventSubscriptionDeliveryRun {
    pub accepted: usize,
    pub deduplicated: usize,
    pub skipped: usize,
    pub blocked: bool,
    pub lagged: bool,
    /// Private Store position reached by this lower scheduler sweep.
    pub(crate) through: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct StoredSubscription {
    pub(crate) recipient_ref: String,
    pub(crate) subject: String,
    pub(crate) generation: u64,
    pub(crate) state: String,
    pub(crate) store_id: String,
    pub(crate) restore_generation: u64,
    pub(crate) placement_kind: String,
    pub(crate) placement_ref: String,
    pub(crate) placement_token: [u8; 32],
    pub(crate) scan_position: u64,
    pub(crate) created_by: String,
}

impl SessionStore {
    /// Enable or disable one durable future-only Event subscription.
    ///
    /// Repeating enabled=true for an already-active subscription with current
    /// Store placement is a zero-write no-op. Re-enabling disabled/lagged/stale
    /// state mints one fresh generation and atomically rebaselines at the
    /// current safe Event tail.
    pub fn events_subscribe(
        &self,
        actor: &PrincipalId,
        subject: &str,
        recipient_ref: &str,
        enabled: bool,
    ) -> Result<EventSubscriptionUpdate> {
        let pattern = EventPattern::parse_absolute(subject)
            .map_err(|_| Error::Denied("invalid Event subscription subject"))?;
        let placement = EventSelectorPlacement::from_pattern(&pattern)?;

        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (recipient_session, recipient_branch) = agent_owner::key_for_ref(&tx, recipient_ref)?;
        let recipient = agent_owner::AgentKey::new(&recipient_session, &recipient_branch);
        let (evidence, permission) = authorize_management(&tx, actor, recipient)?;

        if enabled {
            let (lifecycle, _) = agent_owner::lifecycle(&tx, recipient)?;
            if lifecycle != "open" {
                return Err(Error::Conflict("Event subscription recipient must be open"));
            }
            event_placement::admit(&tx, actor, &placement)?;
        }

        let existing = subscription_row(&tx, recipient_ref, pattern.as_str())?;
        if !enabled {
            let Some(existing) = existing else {
                db(tx.commit())?;
                return Ok(update(
                    recipient_ref,
                    pattern.as_str(),
                    false,
                    false,
                    None,
                    None,
                ));
            };
            if existing.state == "disabled" {
                db(tx.commit())?;
                return Ok(update(
                    recipient_ref,
                    pattern.as_str(),
                    false,
                    false,
                    Some(existing.generation),
                    None,
                ));
            }
            let changed = db(tx.execute(
                "UPDATE event_subscriptions_current
                 SET state='disabled',lag_reason=NULL,updated_at=?1
                 WHERE recipient_ref=?2 AND subject=?3 AND generation=?4
                   AND state IN ('active','lagged')",
                params![now(), recipient_ref, pattern.as_str(), existing.generation],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict("Event subscription changed concurrently"));
            }
            db(tx.commit())?;
            return Ok(update(
                recipient_ref,
                pattern.as_str(),
                false,
                true,
                Some(existing.generation),
                None,
            ));
        }

        let projection = event_projection::snapshot(&tx)?;
        if !projection.usable {
            return Err(Error::Storage(
                "Event projection is unavailable; owner normalization required",
            ));
        }
        let placement_token = placement.token(&projection.scope);
        let current_affinity = existing.as_ref().is_some_and(|row| {
            row.state == "active"
                && row.store_id == projection.scope.id().as_str()
                && row.restore_generation == projection.scope.restore_generation()
                && row.placement_kind == placement.kind()
                && row.placement_ref == placement.reference()
                && row.placement_token == placement_token
        });
        if current_affinity {
            let existing = existing.expect("checked existing active subscription");
            db(tx.commit())?;
            return Ok(update(
                recipient_ref,
                pattern.as_str(),
                true,
                false,
                Some(existing.generation),
                Some(EventSubscriptionState::Active),
            ));
        }

        let generation = existing
            .as_ref()
            .map(|row| {
                row.generation
                    .checked_add(1)
                    .ok_or(Error::Limit("Event subscription generation exhausted"))
            })
            .transpose()?
            .unwrap_or(1);
        let at = now();
        let baseline = projection.indexed_through.get();

        if existing.is_some() {
            let changed = db(tx.execute(
                "UPDATE event_subscriptions_current
                 SET placement_kind=?1,placement_ref=?2,placement_token=?3,
                     store_id=?4,restore_generation=?5,generation=?6,
                     baseline_position=?7,scan_position=?7,
                     created_by=?8,authority_ref=?9,recipient_permission=?10,
                     recipient_access_generation=?11,recipient_grant_revision=?12,
                     recipient_acl_revision=?13,recipient_is_owner=?14,
                     state='active',lag_reason=NULL,created_at=?15,updated_at=?15
                 WHERE recipient_ref=?16 AND subject=?17",
                params![
                    placement.kind(),
                    placement.reference(),
                    placement_token.as_slice(),
                    projection.scope.id().as_str(),
                    projection.scope.restore_generation(),
                    generation,
                    baseline,
                    actor.as_str(),
                    HOST_AUTHORITY_REF,
                    permission,
                    evidence.access_generation.as_slice(),
                    evidence.grant_revision,
                    evidence.acl_revision,
                    i64::from(evidence.owner),
                    at,
                    recipient_ref,
                    pattern.as_str(),
                ],
            ))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "Event subscription rebaseline changed concurrently",
                ));
            }
        } else {
            db(tx.execute(
                "INSERT INTO event_subscriptions_current(
                     recipient_ref,subject,placement_kind,placement_ref,placement_token,
                     store_id,restore_generation,generation,baseline_position,scan_position,
                     created_by,authority_ref,recipient_permission,recipient_access_generation,
                     recipient_grant_revision,recipient_acl_revision,recipient_is_owner,
                     state,lag_reason,created_at,updated_at
                 ) VALUES(
                     ?1,?2,?3,?4,?5,?6,?7,1,?8,?8,
                     ?9,?10,?11,?12,?13,?14,?15,'active',NULL,?16,?16
                 )",
                params![
                    recipient_ref,
                    pattern.as_str(),
                    placement.kind(),
                    placement.reference(),
                    placement_token.as_slice(),
                    projection.scope.id().as_str(),
                    projection.scope.restore_generation(),
                    baseline,
                    actor.as_str(),
                    HOST_AUTHORITY_REF,
                    permission,
                    evidence.access_generation.as_slice(),
                    evidence.grant_revision,
                    evidence.acl_revision,
                    i64::from(evidence.owner),
                    at,
                ],
            ))?;
        }
        db(tx.commit())?;
        Ok(update(
            recipient_ref,
            pattern.as_str(),
            true,
            true,
            Some(generation),
            Some(EventSubscriptionState::Active),
        ))
    }

    /// Management-only bounded inspection. after is the exact subject from the
    /// previous page, matching the crate's existing canonical-key paging style;
    /// disabled tombstones and private scan/baseline positions stay hidden.
    pub fn events_subscriptions_list(
        &self,
        actor: &PrincipalId,
        recipient_ref: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EventSubscriptionView>> {
        if limit == 0 || limit > 256 {
            return Err(Error::Limit("invalid Event subscription list limit"));
        }
        let connection = self.connection()?;
        let (session, branch) = agent_owner::key_for_ref(&connection, recipient_ref)?;
        let recipient = agent_owner::AgentKey::new(&session, &branch);
        authorize_management(&connection, actor, recipient)?;

        if let Some(subject) = after {
            let exists: bool = db(connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM event_subscriptions_current
                     WHERE recipient_ref=?1 AND subject=?2
                       AND state IN ('active','lagged')
                 )",
                params![recipient_ref, subject],
                |row| row.get(0),
            ))?;
            if !exists {
                return Err(Error::Denied(
                    "Event subscription list cursor is unavailable",
                ));
            }
        }

        let mut query = db(connection.prepare(
            "SELECT subject,generation,state,created_by,authority_ref,created_at
             FROM event_subscriptions_current
             WHERE recipient_ref=?1 AND state IN ('active','lagged')
               AND (?2 IS NULL OR subject>?2)
             ORDER BY subject
             LIMIT ?3",
        ))?;
        let rows = db(
            query.query_map(params![recipient_ref, after, limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            }),
        )?;
        db(rows
            .map(|row| {
                let (subject, generation, state, created_by, authority_ref, created_at) = row?;
                let state = match state.as_str() {
                    "active" => EventSubscriptionState::Active,
                    "lagged" => EventSubscriptionState::Lagged,
                    _ => {
                        return Err(rusqlite::Error::InvalidColumnType(
                            2,
                            "state".into(),
                            rusqlite::types::Type::Text,
                        ))
                    }
                };
                Ok(EventSubscriptionView {
                    recipient_ref: recipient_ref.to_owned(),
                    subject,
                    generation,
                    state,
                    created_by,
                    authority_ref,
                    created_at,
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>())
    }

    /// Lower host-scheduler seam: consume one bounded subscription window.
    ///
    /// This creates no worker and performs no polling. The caller decides when
    /// to invoke it; EventScan supplies ordered candidates and EventInput owns
    /// the only acceptance/dedupe linearization transaction.
    pub fn event_subscription_deliver(
        &self,
        recipient_ref: &str,
        subject: &str,
        limit: usize,
    ) -> Result<EventSubscriptionDeliveryRun> {
        if limit == 0 || limit > 256 {
            return Err(Error::Limit(
                "Event subscription delivery limit must be 1..256",
            ));
        }
        let pattern = EventPattern::parse_absolute(subject)
            .map_err(|_| Error::Denied("invalid Event subscription subject"))?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let Some(subscription) = subscription_row(&tx, recipient_ref, pattern.as_str())? else {
            return Err(Error::Denied("Event subscription is unavailable"));
        };
        let mut run = EventSubscriptionDeliveryRun {
            through: subscription.scan_position,
            lagged: subscription.state == "lagged",
            ..Default::default()
        };
        if subscription.state != "active" {
            db(tx.commit())?;
            return Ok(run);
        }

        let placement = EventSelectorPlacement::from_stored(
            &subscription.placement_kind,
            subscription.placement_ref.clone(),
        )?;
        let parsed_placement = EventSelectorPlacement::from_pattern(&pattern)?;
        if parsed_placement != placement {
            return Err(Error::Storage(
                "Event subscription pattern disagrees with stored placement",
            ));
        }

        let scope = StoreScope::load(&tx)?;
        if subscription.store_id != scope.id().as_str()
            || subscription.restore_generation != scope.restore_generation()
            || subscription.placement_token != placement.token(&scope)
        {
            lag_subscription_in(&tx, &subscription, "store_scope_stale")?;
            db(tx.commit())?;
            run.lagged = true;
            return Ok(run);
        }
        let placement_snapshot = match event_placement::snapshot(&tx, &placement) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                lag_subscription_in(&tx, &subscription, "placement_stale")?;
                db(tx.commit())?;
                run.lagged = true;
                return Ok(run);
            }
            Err(Error::Conflict(_)) => {
                run.blocked = true;
                db(tx.commit())?;
                return Ok(run);
            }
            Err(other) => return Err(other),
        };
        let start = StorePos::new(subscription.scan_position);
        if event_retention::expired_now(&tx, start)? {
            lag_subscription_in(&tx, &subscription, "retention_overtook_scan")?;
            db(tx.commit())?;
            run.lagged = true;
            return Ok(run);
        }

        let creator = PrincipalId::new(subscription.created_by.clone())?;
        let fence = EventScanFence::capture(&tx)?;
        let mut position = start;
        let mut examined = 0usize;
        let mut terminal = 0usize;
        let mut authority_cache =
            HashMap::<String, EventAuthorization<agent_owner::AgentAccessEvidence>>::new();

        while terminal < limit && examined < MAX_EVENT_SCAN_ROWS {
            let remaining = MAX_EVENT_SCAN_ROWS - examined;
            let result = event_scan::scan_events(
                &tx,
                &fence,
                position,
                &pattern,
                remaining,
                || event_placement::state(&tx, &placement, &placement_snapshot),
                |source| {
                    event_authority::source_visibility(
                        &tx,
                        &creator,
                        &placement,
                        source,
                        &mut authority_cache,
                    )
                },
                |candidate, _source_evidence| event_authority::candidate_authority(candidate),
            )?;
            match result {
                EventScanResult::Candidate {
                    candidate,
                    examined: scanned,
                } => {
                    examined = examined.saturating_add(scanned);
                    let event_scan::EventScanCandidate {
                        event,
                        source_evidence,
                        candidate_evidence,
                    } = *candidate;
                    match event_input::admit_subscription_candidate(
                        &tx,
                        &subscription,
                        &event,
                        &source_evidence,
                        &candidate_evidence,
                    )? {
                        event_input::EventInputAdmission::Accepted { .. } => {
                            run.accepted += 1;
                            terminal += 1;
                            position = event.position;
                        }
                        event_input::EventInputAdmission::Deduplicated { .. } => {
                            run.deduplicated += 1;
                            terminal += 1;
                            position = event.position;
                        }
                        event_input::EventInputAdmission::ProvenDenied => {
                            run.skipped += 1;
                            terminal += 1;
                            position = event.position;
                        }
                        event_input::EventInputAdmission::AuthorizationUnavailable => {
                            run.blocked = true;
                            break;
                        }
                        event_input::EventInputAdmission::PlacementStale => {
                            lag_subscription_in(&tx, &subscription, "placement_stale")?;
                            run.lagged = true;
                            break;
                        }
                    }
                }
                EventScanResult::ProvenSkip {
                    through,
                    examined: scanned,
                } => {
                    examined = examined.saturating_add(scanned);
                    run.skipped = run.skipped.saturating_add(scanned);
                    position = through;
                }
                EventScanResult::BlockedUnknown => {
                    run.blocked = true;
                    break;
                }
                EventScanResult::Stale => {
                    lag_subscription_in(&tx, &subscription, "placement_stale")?;
                    run.lagged = true;
                    break;
                }
                EventScanResult::End { through } => {
                    position = through;
                    break;
                }
            }
        }

        if !run.lagged && position != start {
            advance_scan_in(&tx, &subscription, start, position)?;
        }
        run.through = position.get();
        db(tx.commit())?;
        Ok(run)
    }
}

fn advance_scan_in(
    connection: &rusqlite::Connection,
    subscription: &StoredSubscription,
    expected: StorePos,
    through: StorePos,
) -> Result<()> {
    let changed = db(connection.execute(
        "UPDATE event_subscriptions_current
         SET scan_position=?1,updated_at=?2
         WHERE recipient_ref=?3 AND subject=?4 AND generation=?5
           AND state='active' AND scan_position=?6
           AND store_id=?7 AND restore_generation=?8",
        params![
            through.get(),
            now(),
            subscription.recipient_ref,
            subscription.subject,
            subscription.generation,
            expected.get(),
            subscription.store_id,
            subscription.restore_generation,
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Event subscription scan progress changed concurrently",
        ));
    }
    Ok(())
}

fn lag_subscription_in(
    connection: &rusqlite::Connection,
    subscription: &StoredSubscription,
    reason: &str,
) -> Result<()> {
    if reason.is_empty() || reason.len() > 256 {
        return Err(Error::Storage("invalid Event subscription lag reason"));
    }
    let changed = db(connection.execute(
        "UPDATE event_subscriptions_current
         SET state='lagged',lag_reason=?1,updated_at=?2
         WHERE recipient_ref=?3 AND subject=?4 AND generation=?5 AND state='active'",
        params![
            reason,
            now(),
            subscription.recipient_ref,
            subscription.subject,
            subscription.generation,
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "Event subscription lag state changed concurrently",
        ));
    }
    Ok(())
}

/// Recipient lifecycle fence used by the Agent close owners.
///
/// Disabled tombstones retain the last generation so a later explicit re-enable
/// must mint a newer generation; an old in-flight admission can therefore never
/// become valid again after close/disable.
pub(crate) fn disable_recipient_in(
    connection: &rusqlite::Connection,
    recipient_ref: &str,
) -> Result<usize> {
    db(connection.execute(
        "UPDATE event_subscriptions_current
         SET state='disabled',lag_reason=NULL,updated_at=?1
         WHERE recipient_ref=?2 AND state IN ('active','lagged')",
        params![now(), recipient_ref],
    ))
}

pub(crate) fn subscription_row(
    connection: &rusqlite::Connection,
    recipient_ref: &str,
    subject: &str,
) -> Result<Option<StoredSubscription>> {
    db(connection
        .query_row(
            "SELECT recipient_ref,subject,generation,state,store_id,restore_generation,
                    placement_kind,placement_ref,placement_token,scan_position,created_by
             FROM event_subscriptions_current
             WHERE recipient_ref=?1 AND subject=?2",
            params![recipient_ref, subject],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Vec<u8>>(8)?,
                    row.get::<_, u64>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional())?
    .map(
        |(
            recipient_ref,
            subject,
            generation,
            state,
            store_id,
            restore_generation,
            placement_kind,
            placement_ref,
            placement_token,
            scan_position,
            created_by,
        )| {
            if !matches!(state.as_str(), "active" | "lagged" | "disabled")
                || !matches!(placement_kind.as_str(), "agent" | "project")
            {
                return Err(Error::Storage("invalid Event subscription state"));
            }
            let placement_token: [u8; 32] = placement_token
                .try_into()
                .map_err(|_| Error::Storage("invalid Event subscription placement token"))?;
            Ok(StoredSubscription {
                recipient_ref,
                subject,
                generation,
                state,
                store_id,
                restore_generation,
                placement_kind,
                placement_ref,
                placement_token,
                scan_position,
                created_by,
            })
        },
    )
    .transpose()
}

pub(crate) fn authorize_management(
    connection: &rusqlite::Connection,
    actor: &PrincipalId,
    recipient: agent_owner::AgentKey<'_>,
) -> Result<(agent_owner::AgentAccessEvidence, &'static str)> {
    match agent_owner::authorize_permission(
        connection,
        actor,
        recipient,
        agent_owner::AgentPermission::Control,
    ) {
        Ok(evidence) => Ok((evidence, "control")),
        Err(Error::Denied(_)) => agent_owner::authorize_permission(
            connection,
            actor,
            recipient,
            agent_owner::AgentPermission::Manage,
        )
        .map(|evidence| (evidence, "manage")),
        Err(other) => Err(other),
    }
}

fn update(
    recipient_ref: &str,
    subject: &str,
    enabled: bool,
    changed: bool,
    generation: Option<u64>,
    state: Option<EventSubscriptionState>,
) -> EventSubscriptionUpdate {
    EventSubscriptionUpdate {
        recipient_ref: recipient_ref.to_owned(),
        subject: subject.to_owned(),
        enabled,
        changed,
        generation,
        state,
    }
}
