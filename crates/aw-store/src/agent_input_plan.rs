//! Mutable host-staged Agent input plan.
//!
//! Staged drafts are host state with one owner-local revision. Runtime custody
//! stays in ActorQueue/AuthoritySteer after the later Send boundary.

use crate::{
    actor_queue, agent_owner, db, durability, now, session_in, Error, PrincipalId, Result,
    SecretStore, SessionStore,
};
use getrandom::fill;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ITEMS: usize = 256;
const ORDER_STEP: i64 = 1024;
const REBALANCE_SHIFT: i64 = 1_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentInputIntent {
    Followup { after: Option<u64> },
    Steer { turn: u64 },
    Interrupt { turn: u64 },
}

impl AgentInputIntent {
    fn stored(&self) -> (&'static str, Option<u64>) {
        match self {
            Self::Followup { after } => ("followup", *after),
            Self::Steer { turn } => ("steer", Some(*turn)),
            Self::Interrupt { turn } => ("interrupt", Some(*turn)),
        }
    }

    fn from_stored(kind: &str, turn: Option<u64>) -> Result<Self> {
        match (kind, turn) {
            ("followup", after) => Ok(Self::Followup { after }),
            ("steer", Some(turn)) if turn > 0 => Ok(Self::Steer { turn }),
            ("interrupt", Some(turn)) if turn > 0 => Ok(Self::Interrupt { turn }),
            _ => Err(Error::Storage("invalid staged Agent input intent")),
        }
    }

    fn permission(&self) -> agent_owner::AgentPermission {
        match self {
            Self::Followup { .. } => agent_owner::AgentPermission::Contribute,
            Self::Steer { .. } | Self::Interrupt { .. } => agent_owner::AgentPermission::Control,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInputAdd {
    pub text: String,
    pub intent: AgentInputIntent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInputEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<AgentInputIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInputPlanItem {
    pub id: String,
    pub text: String,
    pub intent: AgentInputIntent,
    pub creator: String,
    pub source: String,
    pub last_editor: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInputPlanSnapshot {
    pub agent_ref: String,
    pub inputs_rev: u64,
    pub items: Vec<AgentInputPlanItem>,
}

#[derive(Clone)]
struct StoredItem {
    id: String,
    intent: AgentInputIntent,
    order: i64,
    creator: String,
    last_editor: String,
    payload_ref: String,
    payload_sha256: String,
    created_at: i64,
    updated_at: i64,
}

fn table_exists(connection: &Connection) -> Result<bool> {
    db(connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type='table' AND name='agent_input_plans'
         )",
        [],
        |row| row.get(0),
    ))
}

pub(crate) fn ensure_for_agent_if_available(
    connection: &Connection,
    session: &str,
    branch: &str,
) -> Result<()> {
    if !table_exists(connection)? {
        return Ok(());
    }
    db(connection.execute(
        "INSERT INTO agent_input_plans(session_id,branch_id,inputs_rev)
         VALUES(?1,?2,1) ON CONFLICT(session_id,branch_id) DO NOTHING",
        params![session, branch],
    ))?;
    Ok(())
}

fn validate_text(text: &str) -> Result<()> {
    if text.is_empty()
        || text.len() > 64 * 1024
        || text.contains('\0')
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::Denied(
            "staged Agent input text must contain 1..65536 safe bytes",
        ));
    }
    Ok(())
}

fn mint_id() -> Result<String> {
    let mut entropy = [0u8; 32];
    fill(&mut entropy).map_err(|_| Error::Storage("OS randomness unavailable for staged input"))?;
    Ok(format!("q:{:x}", Sha256::digest(entropy)))
}

fn canonical_principal(domain: &str, user: &str) -> String {
    format!("/d:{domain}/i:{user}")
}

fn agent_key(
    connection: &Connection,
    actor: &PrincipalId,
    reference: &str,
    permission: agent_owner::AgentPermission,
) -> Result<(String, String, String)> {
    let (session, branch) = agent_owner::key_for_ref(connection, reference)?;
    agent_owner::authorize_permission(
        connection,
        actor,
        agent_owner::AgentKey::new(&session, &branch),
        permission,
    )?;
    let domain: String = db(connection.query_row(
        "SELECT domain_handle FROM agent_state_current
         WHERE session_id=?1 AND branch_id=?2",
        params![session, branch],
        |row| row.get(0),
    ))?;
    Ok((session, branch, domain))
}

fn require_open(connection: &Connection, session: &str, branch: &str) -> Result<()> {
    let (lifecycle, _) =
        agent_owner::lifecycle(connection, agent_owner::AgentKey::new(session, branch))?;
    if lifecycle != "open" {
        return Err(Error::Conflict(
            "staged Agent input plan is frozen after close begins",
        ));
    }
    Ok(())
}

fn validate_intent(
    connection: &Connection,
    session: &str,
    branch: &str,
    intent: &AgentInputIntent,
) -> Result<()> {
    let session_info = session_in(connection, session)?;
    let active: Option<u64> = db(connection
        .query_row(
            "SELECT r.turn_id
             FROM branches b JOIN runs r ON r.id=b.active_run
             WHERE b.session_id=?1 AND b.id=?2
               AND r.session_id=?1 AND r.branch_id=?2 AND r.status='running'",
            params![session, branch],
            |row| row.get(0),
        )
        .optional())?;
    match intent {
        AgentInputIntent::Followup { after: None } => Ok(()),
        AgentInputIntent::Followup { after: Some(turn) }
            if *turn > 0 && *turn < session_info.next_turn =>
        {
            Ok(())
        }
        AgentInputIntent::Steer { turn } | AgentInputIntent::Interrupt { turn }
            if active == Some(*turn) =>
        {
            Ok(())
        }
        AgentInputIntent::Followup { .. } => {
            Err(Error::Conflict("followup target Turn is not yet known"))
        }
        AgentInputIntent::Steer { .. } | AgentInputIntent::Interrupt { .. } => Err(
            Error::Conflict("steer/interrupt target is not the exact active Turn"),
        ),
    }
}

fn plan_rev(connection: &Connection, session: &str, branch: &str) -> Result<u64> {
    db(connection.query_row(
        "SELECT inputs_rev FROM agent_input_plans WHERE session_id=?1 AND branch_id=?2",
        params![session, branch],
        |row| row.get(0),
    ))
}

fn load_items(connection: &Connection, session: &str, branch: &str) -> Result<Vec<StoredItem>> {
    let mut query = db(connection.prepare(
        "SELECT input_id,intent_kind,target_turn,order_key,creator_handle,last_editor_handle,
                payload_ref,payload_sha256,created_at,updated_at
         FROM agent_input_plan_items
         WHERE session_id=?1 AND branch_id=?2
         ORDER BY order_key,input_id LIMIT ?3",
    ))?;
    let rows = db(
        query.query_map(params![session, branch, (MAX_ITEMS + 1) as u64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<u64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
            ))
        }),
    )?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() > MAX_ITEMS {
        return Err(Error::Limit("staged Agent input plan exceeds bound"));
    }
    rows.into_iter()
        .map(
            |(
                id,
                kind,
                turn,
                order,
                creator,
                editor,
                payload_ref,
                payload_sha256,
                created,
                updated,
            )| {
                Ok(StoredItem {
                    id,
                    intent: AgentInputIntent::from_stored(&kind, turn)?,
                    order,
                    creator,
                    last_editor: editor,
                    payload_ref,
                    payload_sha256,
                    created_at: created,
                    updated_at: updated,
                })
            },
        )
        .collect()
}

fn require_rev(current: u64, expected: Option<u64>) -> Result<()> {
    if expected.is_some_and(|expected| expected != current) {
        return Err(Error::Conflict("stale Agent input-plan revision"));
    }
    Ok(())
}

fn require_item_permission(
    connection: &Connection,
    actor: &PrincipalId,
    agent: agent_owner::AgentKey<'_>,
    actor_user: &str,
    creator: &str,
    intent: &AgentInputIntent,
) -> Result<()> {
    agent_owner::authorize_permission(connection, actor, agent, intent.permission())?;
    if creator != actor_user {
        agent_owner::authorize_permission(
            connection,
            actor,
            agent,
            agent_owner::AgentPermission::Manage,
        )?;
    }
    Ok(())
}

fn next_rev(connection: &Connection, session: &str, branch: &str, current: u64) -> Result<u64> {
    let next = current
        .checked_add(1)
        .ok_or(Error::Limit("Agent input-plan revision exhausted"))?;
    let changed = db(connection.execute(
        "UPDATE agent_input_plans SET inputs_rev=?1
         WHERE session_id=?2 AND branch_id=?3 AND inputs_rev=?4",
        params![next, session, branch, current],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict("Agent input plan changed concurrently"));
    }
    Ok(next)
}

fn publish_changed(
    connection: &Connection,
    actor: &PrincipalId,
    session: &str,
    branch: &str,
    inputs_rev: u64,
) -> Result<()> {
    let canonical =
        agent_owner::canonical_ref(connection, agent_owner::AgentKey::new(session, branch))?;
    let txid = durability::tx_id()?;
    durability::commit_event(
        connection,
        &txid,
        "agent.changed",
        "agent",
        &canonical,
        Some(actor),
        serde_json::json!({"aspect":"inputs","inputs_rev":inputs_rev}),
    )
}

fn rebalance(connection: &Connection, session: &str, branch: &str) -> Result<()> {
    let items = load_items(connection, session, branch)?;
    db(connection.execute(
        "UPDATE agent_input_plan_items SET order_key=order_key+?1
         WHERE session_id=?2 AND branch_id=?3",
        params![REBALANCE_SHIFT, session, branch],
    ))?;
    for (index, item) in items.iter().enumerate() {
        let order = i64::try_from(index + 1)
            .map_err(|_| Error::Limit("staged input order overflow"))?
            .checked_mul(ORDER_STEP)
            .ok_or(Error::Limit("staged input order overflow"))?;
        db(connection.execute(
            "UPDATE agent_input_plan_items SET order_key=?1
             WHERE session_id=?2 AND branch_id=?3 AND input_id=?4",
            params![order, session, branch, item.id],
        ))?;
    }
    Ok(())
}

fn insertion_order(
    connection: &Connection,
    session: &str,
    branch: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<i64> {
    if before.is_some() && after.is_some() {
        return Err(Error::Denied(
            "staged input position accepts only one of before or after",
        ));
    }
    let items = load_items(connection, session, branch)?;
    if before.is_none() && after.is_none() {
        return items
            .last()
            .map(|item| {
                item.order
                    .checked_add(ORDER_STEP)
                    .ok_or(Error::Limit("staged input order exhausted"))
            })
            .transpose()
            .map(|value| value.unwrap_or(ORDER_STEP));
    }
    let anchor = before.or(after).expect("position anchor checked");
    let index = items
        .iter()
        .position(|item| item.id == anchor)
        .ok_or(Error::Denied("staged input position anchor does not exist"))?;
    let (left, right) = if before.is_some() {
        (
            index.checked_sub(1).map(|i| items[i].order).unwrap_or(0),
            items[index].order,
        )
    } else {
        (
            items[index].order,
            items
                .get(index + 1)
                .map(|item| item.order)
                .unwrap_or_else(|| items[index].order.saturating_add(ORDER_STEP * 2)),
        )
    };
    if right - left > 1 {
        return Ok(left + (right - left) / 2);
    }
    rebalance(connection, session, branch)?;
    insertion_order(connection, session, branch, before, after)
}

impl SessionStore {
    pub fn agent_inputs_list(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
    ) -> Result<AgentInputPlanSnapshot> {
        let (rev, items, domain) = {
            let connection = self.connection()?;
            let (session, branch, domain) = agent_key(
                &connection,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            (
                plan_rev(&connection, &session, &branch)?,
                load_items(&connection, &session, &branch)?,
                domain,
            )
        };
        let vault = self.local_secret_store()?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let bytes = vault.get(&item.payload_ref)?;
            let text = String::from_utf8(bytes.to_vec())
                .map_err(|_| Error::Storage("staged Agent input payload is not UTF-8"))?;
            if format!("{:x}", Sha256::digest(text.as_bytes())) != item.payload_sha256 {
                return Err(Error::Storage(
                    "staged Agent input payload digest no longer matches",
                ));
            }
            out.push(AgentInputPlanItem {
                id: item.id,
                text,
                intent: item.intent,
                creator: canonical_principal(&domain, &item.creator),
                source: "host".into(),
                last_editor: canonical_principal(&domain, &item.last_editor),
                created_at: item.created_at,
                updated_at: item.updated_at,
            });
        }
        Ok(AgentInputPlanSnapshot {
            agent_ref: canonical_ref.to_owned(),
            inputs_rev: rev,
            items: out,
        })
    }

    pub fn agent_inputs_add(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        additions: &[AgentInputAdd],
        expected_rev: Option<u64>,
    ) -> Result<AgentInputPlanSnapshot> {
        if additions.is_empty() || additions.len() > MAX_ITEMS {
            return Err(Error::Limit("invalid staged Agent input add count"));
        }
        let actor_user = self.current_user(actor)?.user_id;
        let mut custody = Vec::with_capacity(additions.len());
        let mut ids = Vec::with_capacity(additions.len());
        for addition in additions {
            validate_text(&addition.text)?;
            let id = mint_id()?;
            let prepared = actor_queue::mint_prepared_queue_custody("staged", &id, &addition.text)?;
            self.write_prepared_queue_custody(&prepared, &addition.text)?;
            ids.push(id);
            custody.push(prepared);
        }
        let result = (|| {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (session, branch, _) = agent_key(
                &tx,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            require_open(&tx, &session, &branch)?;
            let current = plan_rev(&tx, &session, &branch)?;
            require_rev(current, expected_rev)?;
            let existing = load_items(&tx, &session, &branch)?;
            if existing.len().saturating_add(additions.len()) > MAX_ITEMS {
                return Err(Error::Limit("staged Agent input plan capacity reached"));
            }
            let agent = agent_owner::AgentKey::new(&session, &branch);
            for addition in additions {
                agent_owner::authorize_permission(&tx, actor, agent, addition.intent.permission())?;
                validate_intent(&tx, &session, &branch, &addition.intent)?;
                if (addition.before.is_some() || addition.after.is_some()) && expected_rev.is_none()
                {
                    return Err(Error::Conflict(
                        "relative staged input insertion requires inputs_rev",
                    ));
                }
            }
            let when = now();
            for (index, addition) in additions.iter().enumerate() {
                let order = insertion_order(
                    &tx,
                    &session,
                    &branch,
                    addition.before.as_deref(),
                    addition.after.as_deref(),
                )?;
                let (kind, turn) = addition.intent.stored();
                db(tx.execute(
                    "INSERT INTO agent_input_plan_items(
                         session_id,branch_id,input_id,intent_kind,target_turn,order_key,
                         creator_handle,source_kind,last_editor_handle,payload_ref,payload_sha256,
                         created_at,updated_at
                     ) VALUES(?1,?2,?3,?4,?5,?6,?7,'host',?7,?8,?9,?10,?10)",
                    params![
                        session,
                        branch,
                        ids[index],
                        kind,
                        turn,
                        order,
                        actor_user,
                        custody[index].payload_ref,
                        custody[index].payload_sha256,
                        when
                    ],
                ))?;
            }
            let next = next_rev(&tx, &session, &branch, current)?;
            publish_changed(&tx, actor, &session, &branch, next)?;
            db(tx.commit())?;
            Ok(())
        })();
        if result.is_err() {
            for prepared in &custody {
                let _ = self.discard_prepared_queue_custody(prepared);
            }
        }
        result?;
        self.agent_inputs_list(actor, canonical_ref)
    }

    pub fn agent_inputs_edit(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        id: &str,
        edit: AgentInputEdit,
        expected_rev: u64,
    ) -> Result<AgentInputPlanSnapshot> {
        if edit.text.is_none()
            && edit.intent.is_none()
            && edit.before.is_none()
            && edit.after.is_none()
        {
            return Err(Error::Denied("staged input edit has no changes"));
        }
        if edit.before.is_some() && edit.after.is_some() {
            return Err(Error::Denied(
                "staged input edit accepts only one of before or after",
            ));
        }
        if edit.before.as_deref() == Some(id) || edit.after.as_deref() == Some(id) {
            return Err(Error::Denied("staged input cannot move relative to itself"));
        }
        let actor_user = self.current_user(actor)?.user_id;
        let prepared = edit
            .text
            .as_deref()
            .map(|text| {
                validate_text(text)?;
                let prepared = actor_queue::mint_prepared_queue_custody("staged", id, text)?;
                self.write_prepared_queue_custody(&prepared, text)?;
                Ok::<_, Error>(prepared)
            })
            .transpose()?;
        let mut old_custody = None;
        let result = (|| {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (session, branch, _) = agent_key(
                &tx,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            require_open(&tx, &session, &branch)?;
            let current = plan_rev(&tx, &session, &branch)?;
            require_rev(current, Some(expected_rev))?;
            let items = load_items(&tx, &session, &branch)?;
            let old = items
                .iter()
                .find(|item| item.id == id)
                .cloned()
                .ok_or(Error::Denied("staged input item does not exist"))?;
            let next_intent = edit.intent.clone().unwrap_or_else(|| old.intent.clone());
            let agent = agent_owner::AgentKey::new(&session, &branch);
            require_item_permission(&tx, actor, agent, &actor_user, &old.creator, &old.intent)?;
            require_item_permission(&tx, actor, agent, &actor_user, &old.creator, &next_intent)?;
            validate_intent(&tx, &session, &branch, &next_intent)?;
            let order = if edit.before.is_some() || edit.after.is_some() {
                insertion_order(
                    &tx,
                    &session,
                    &branch,
                    edit.before.as_deref(),
                    edit.after.as_deref(),
                )?
            } else {
                old.order
            };
            let (kind, turn) = next_intent.stored();
            let (payload_ref, payload_sha256) = prepared
                .as_ref()
                .map(|prepared| {
                    (
                        prepared.payload_ref.as_str(),
                        prepared.payload_sha256.as_str(),
                    )
                })
                .unwrap_or((&old.payload_ref, &old.payload_sha256));
            db(tx.execute(
                "UPDATE agent_input_plan_items
                 SET intent_kind=?1,target_turn=?2,order_key=?3,last_editor_handle=?4,
                     payload_ref=?5,payload_sha256=?6,updated_at=?7
                 WHERE session_id=?8 AND branch_id=?9 AND input_id=?10",
                params![
                    kind,
                    turn,
                    order,
                    actor_user,
                    payload_ref,
                    payload_sha256,
                    now(),
                    session,
                    branch,
                    id
                ],
            ))?;
            let next = next_rev(&tx, &session, &branch, current)?;
            publish_changed(&tx, actor, &session, &branch, next)?;
            if prepared.is_some() {
                old_custody = Some(actor_queue::PreparedQueueCustody {
                    payload_ref: old.payload_ref,
                    payload_sha256: old.payload_sha256,
                });
            }
            db(tx.commit())?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Some(old) = old_custody {
                    let _ = self.discard_prepared_queue_custody(&old);
                }
                self.agent_inputs_list(actor, canonical_ref)
            }
            Err(error) => {
                if let Some(prepared) = &prepared {
                    let _ = self.discard_prepared_queue_custody(prepared);
                }
                Err(error)
            }
        }
    }

    pub fn agent_inputs_delete(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        ids: &[String],
        expected_rev: u64,
    ) -> Result<AgentInputPlanSnapshot> {
        if ids.is_empty() || ids.len() > MAX_ITEMS {
            return Err(Error::Limit("invalid staged Agent input delete count"));
        }
        let unique = ids.iter().collect::<BTreeSet<_>>();
        if unique.len() != ids.len() {
            return Err(Error::Denied("duplicate staged Agent input id"));
        }
        let actor_user = self.current_user(actor)?.user_id;
        let mut removed = Vec::new();
        {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (session, branch, _) = agent_key(
                &tx,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            require_open(&tx, &session, &branch)?;
            let current = plan_rev(&tx, &session, &branch)?;
            require_rev(current, Some(expected_rev))?;
            let items = load_items(&tx, &session, &branch)?;
            let by_id = items
                .iter()
                .map(|item| (item.id.as_str(), item))
                .collect::<BTreeMap<_, _>>();
            let agent = agent_owner::AgentKey::new(&session, &branch);
            for id in ids {
                let item = by_id
                    .get(id.as_str())
                    .ok_or(Error::Denied("staged input item does not exist"))?;
                require_item_permission(
                    &tx,
                    actor,
                    agent,
                    &actor_user,
                    &item.creator,
                    &item.intent,
                )?;
            }
            for id in ids {
                let item = by_id[id.as_str()];
                removed.push(actor_queue::PreparedQueueCustody {
                    payload_ref: item.payload_ref.clone(),
                    payload_sha256: item.payload_sha256.clone(),
                });
                db(tx.execute(
                    "DELETE FROM agent_input_plan_items
                     WHERE session_id=?1 AND branch_id=?2 AND input_id=?3",
                    params![session, branch, id],
                ))?;
            }
            let next = next_rev(&tx, &session, &branch, current)?;
            publish_changed(&tx, actor, &session, &branch, next)?;
            db(tx.commit())?;
        }
        for prepared in &removed {
            let _ = self.discard_prepared_queue_custody(prepared);
        }
        self.agent_inputs_list(actor, canonical_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_store_api::{SessionBackend, SessionBinding};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct PrivateTemp(PathBuf);

    impl PrivateTemp {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "aw-agent-input-plan-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            aw_platform::ensure_private_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn binding(root: &std::path::Path) -> SessionBinding {
        SessionBinding {
            working_directory: root.to_owned(),
            tool_root: root.to_owned(),
            system_prompt: "system".into(),
            profile_hint: "fixture".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    #[test]
    fn v42_staged_input_plan_is_revisioned_encrypted_ordered_and_frozen_on_close() {
        let temp = PrivateTemp::new();
        let store = SessionStore::open(&temp.0).unwrap();
        let actor = PrincipalId::new("internal:fixture:input-plan").unwrap();
        let (session, branch) = store
            .create(&actor, "input plan", binding(&temp.0))
            .unwrap();
        let agent = store
            .agent_binding(&actor, &session.id, &branch.id)
            .unwrap();
        let canonical = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            agent.domain_handle, agent.identity_handle, agent.project_handle, agent.agent_handle
        );

        {
            let mut connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "DROP TABLE agent_input_plan_items;
                     DROP TABLE agent_input_plans;
                     UPDATE store_meta SET value='41' WHERE key='schema_epoch';
                     UPDATE schema_meta
                        SET schema_epoch=41,activation_epoch=41,migration_phase='ready'
                        WHERE singleton=1;
                     UPDATE migration_lease
                        SET holder_client_id=NULL,current_epoch=41,target_epoch=NULL,
                            lease_expires_at=NULL
                        WHERE singleton=1;
                     PRAGMA user_version=41;",
                )
                .unwrap();
            crate::migrate_agent_input_plan(&mut connection).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                42
            );
        }

        let initial_agent_rev = store.agent_get(&actor, &canonical).unwrap().rev;
        let empty = store.agent_inputs_list(&actor, &canonical).unwrap();
        assert_eq!(empty.inputs_rev, 1);
        assert!(empty.items.is_empty());

        let first = store
            .agent_inputs_add(
                &actor,
                &canonical,
                &[AgentInputAdd {
                    text: "first".into(),
                    intent: AgentInputIntent::Followup { after: None },
                    before: None,
                    after: None,
                }],
                None,
            )
            .unwrap();
        assert_eq!(first.inputs_rev, 2);
        assert_eq!(first.items.len(), 1);
        assert!(first.items[0].id.starts_with("q:"));
        assert_eq!(first.items[0].text, "first");
        let first_id = first.items[0].id.clone();

        let second = store
            .agent_inputs_add(
                &actor,
                &canonical,
                &[AgentInputAdd {
                    text: "second".into(),
                    intent: AgentInputIntent::Followup { after: None },
                    before: Some(first_id.clone()),
                    after: None,
                }],
                Some(first.inputs_rev),
            )
            .unwrap();
        assert_eq!(second.inputs_rev, 3);
        assert_eq!(
            second
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            vec!["second", "first"]
        );
        let second_id = second.items[0].id.clone();

        assert!(matches!(
            store.agent_inputs_add(
                &actor,
                &canonical,
                &[AgentInputAdd {
                    text: "stale".into(),
                    intent: AgentInputIntent::Followup { after: None },
                    before: Some(first_id.clone()),
                    after: None,
                }],
                Some(first.inputs_rev),
            ),
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            store.agent_inputs_add(
                &actor,
                &canonical,
                &[AgentInputAdd {
                    text: "no active turn".into(),
                    intent: AgentInputIntent::Steer { turn: 1 },
                    before: None,
                    after: None,
                }],
                Some(second.inputs_rev),
            ),
            Err(Error::Conflict(_))
        ));

        let edited = store
            .agent_inputs_edit(
                &actor,
                &canonical,
                &first_id,
                AgentInputEdit {
                    text: Some("first edited".into()),
                    intent: None,
                    before: Some(second_id.clone()),
                    after: None,
                },
                second.inputs_rev,
            )
            .unwrap();
        assert_eq!(edited.inputs_rev, 4);
        assert_eq!(
            edited
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            vec!["first edited", "second"]
        );

        let deleted = store
            .agent_inputs_delete(
                &actor,
                &canonical,
                std::slice::from_ref(&second_id),
                edited.inputs_rev,
            )
            .unwrap();
        assert_eq!(deleted.inputs_rev, 5);
        assert_eq!(deleted.items.len(), 1);
        assert_eq!(deleted.items[0].id, first_id);
        assert_eq!(
            store.agent_get(&actor, &canonical).unwrap().rev,
            initial_agent_rev,
            "staged plan mutations must not advance Agent rev"
        );

        let current_agent_rev = store.agent_get(&actor, &canonical).unwrap().rev;
        store
            .agent_request_close(&actor, &canonical, Some(current_agent_rev))
            .unwrap();
        let frozen = store.agent_inputs_list(&actor, &canonical).unwrap();
        assert_eq!(frozen.inputs_rev, deleted.inputs_rev);
        assert!(matches!(
            store.agent_inputs_add(
                &actor,
                &canonical,
                &[AgentInputAdd {
                    text: "too late".into(),
                    intent: AgentInputIntent::Followup { after: None },
                    before: None,
                    after: None,
                }],
                Some(frozen.inputs_rev),
            ),
            Err(Error::Conflict(_))
        ));

        let connection = store.connection().unwrap();
        let audit: String = connection
            .query_row(
                "SELECT group_concat(data_json,'\n') FROM audit WHERE session_id=?1",
                [&session.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!audit.contains("first edited"));
        assert!(!audit.contains("second"));
        let changed: u64 = connection
            .query_row(
                "SELECT count(*) FROM commit_log
                 WHERE kind='agent.changed' AND entity_id=?1
                   AND json_extract(payload_json,'$.aspect')='inputs'",
                [&canonical],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(changed, 4);
    }
}
