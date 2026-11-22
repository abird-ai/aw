//! Mutable host-staged Agent input plan.
//!
//! Staged drafts are host state with one owner-local revision. Runtime custody
//! stays in ActorQueue/AuthoritySteer after the later Send boundary.

use crate::{
    actor_queue, agent_owner, db, durability, now, principal_is_session_owner, session_in,
    workspace_writer, Error, PrincipalId, Result, SecretStore, SessionStore,
};
use getrandom::fill;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use zeroize::Zeroizing;

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

#[derive(Clone, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MutationState {
    Prepared,
    Applied,
    Rejected,
    Unknown,
}

impl MutationState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Unknown => "unknown",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            "unknown" => Ok(Self::Unknown),
            _ => Err(Error::Storage("invalid staged Agent input replay state")),
        }
    }
}

#[derive(Clone)]
struct MutationReceipt {
    actor_user: String,
    request_id: String,
    operation: String,
    fingerprint: String,
    session: String,
    branch: String,
    state: MutationState,
    resulting_inputs_rev: Option<u64>,
    result_json: String,
}

pub struct AgentInputSendControl {
    pub request_id: String,
    pub turn: u64,
    pub boundary: aw_agent_core::AuthorityBoundaryKind,
    text: Zeroizing<String>,
}

impl AgentInputSendControl {
    pub fn text(&self) -> &str {
        self.text.as_str()
    }
}

pub struct PreparedAgentInputSend {
    canonical_ref: String,
    session: String,
    branch: String,
    actor_user: String,
    request_id: String,
    fingerprint: String,
    expected_rev: u64,
    selected: Vec<StoredItem>,
    control: Option<AgentInputSendControl>,
    runtime_owner: Option<PrincipalId>,
    recipient_site: Option<String>,
    writer_epoch: Option<u64>,
    prepared_control: Option<crate::AuthoritySteerFact>,
}

impl PreparedAgentInputSend {
    pub fn canonical_ref(&self) -> &str {
        &self.canonical_ref
    }

    pub fn control(&self) -> Option<&AgentInputSendControl> {
        self.control.as_ref()
    }
}

pub enum AgentInputSendStart {
    Replay(AgentInputPlanSnapshot),
    Prepared(Box<PreparedAgentInputSend>),
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

fn validate_request_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(&byte))
    {
        return Err(Error::Denied(
            "invalid staged Agent input mutation identity",
        ));
    }
    Ok(())
}

fn mutation_fingerprint(
    operation: &str,
    canonical_ref: &str,
    semantic: serde_json::Value,
) -> Result<String> {
    let encoded = serde_json::to_vec(&serde_json::json!({
        "version":1,
        "operation":operation,
        "agent":canonical_ref,
        "semantic":semantic,
    }))
    .map_err(|_| Error::Storage("cannot encode staged Agent input mutation fingerprint"))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-agent-input-mutation-v1\0");
    hash.update(encoded);
    Ok(format!("{:x}", hash.finalize()))
}

fn receipt_in(
    connection: &Connection,
    actor_user: &str,
    request_id: &str,
) -> Result<Option<MutationReceipt>> {
    let row: Option<(String, String, String, String, String, String, Option<u64>, String)> = db(
        connection
            .query_row(
                "SELECT operation,fingerprint,session_id,branch_id,state,\n                        actor_user_id,resulting_inputs_rev,result_json\n                 FROM agent_input_mutation_replay\n                 WHERE actor_user_id=?1 AND request_id=?2",
                params![actor_user, request_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional(),
    )?;
    row.map(
        |(operation, fingerprint, session, branch, state, stored_actor, rev, result_json)| {
            Ok(MutationReceipt {
                actor_user: stored_actor,
                request_id: request_id.to_owned(),
                operation,
                fingerprint,
                session,
                branch,
                state: MutationState::parse(&state)?,
                resulting_inputs_rev: rev,
                result_json,
            })
        },
    )
    .transpose()
}

fn validate_replay(
    receipt: &MutationReceipt,
    operation: &str,
    fingerprint: &str,
    session: &str,
    branch: &str,
) -> Result<()> {
    if receipt.operation != operation
        || receipt.fingerprint != fingerprint
        || receipt.session != session
        || receipt.branch != branch
    {
        return Err(Error::Conflict(
            "staged Agent input request identity was reused with changed semantics",
        ));
    }
    Ok(())
}

fn replay_state_in(
    connection: &Connection,
    actor_user: &str,
    request_id: &str,
    operation: &str,
    fingerprint: &str,
    session: &str,
    branch: &str,
) -> Result<Option<MutationState>> {
    let Some(receipt) = receipt_in(connection, actor_user, request_id)? else {
        return Ok(None);
    };
    validate_replay(&receipt, operation, fingerprint, session, branch)?;
    Ok(Some(receipt.state))
}

fn insert_receipt_in(
    connection: &Connection,
    receipt: &MutationReceipt,
    note: Option<&str>,
) -> Result<()> {
    let when = now();
    db(connection.execute(
        "INSERT INTO agent_input_mutation_replay(\n             actor_user_id,request_id,operation,fingerprint,session_id,branch_id,state,\n             resulting_inputs_rev,result_json,note,created_at,updated_at\n         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
        params![
            receipt.actor_user,
            receipt.request_id,
            receipt.operation,
            receipt.fingerprint,
            receipt.session,
            receipt.branch,
            receipt.state.as_str(),
            receipt.resulting_inputs_rev,
            receipt.result_json,
            note,
            when
        ],
    ))?;
    Ok(())
}

fn transition_receipt_in(
    connection: &Connection,
    expected: &MutationReceipt,
    state: MutationState,
    resulting_inputs_rev: Option<u64>,
    result_json: &str,
    note: Option<&str>,
) -> Result<MutationReceipt> {
    let changed = db(connection.execute(
        "UPDATE agent_input_mutation_replay\n         SET state=?1,resulting_inputs_rev=?2,result_json=?3,note=?4,updated_at=?5\n         WHERE actor_user_id=?6 AND request_id=?7 AND operation=?8 AND fingerprint=?9\n           AND session_id=?10 AND branch_id=?11 AND state=?12",
        params![
            state.as_str(),
            resulting_inputs_rev,
            result_json,
            note,
            now(),
            expected.actor_user,
            expected.request_id,
            expected.operation,
            expected.fingerprint,
            expected.session,
            expected.branch,
            expected.state.as_str()
        ],
    ))?;
    if changed != 1 {
        return Err(Error::Conflict(
            "staged Agent input mutation receipt changed concurrently",
        ));
    }
    Ok(MutationReceipt {
        state,
        resulting_inputs_rev,
        result_json: result_json.to_owned(),
        ..expected.clone()
    })
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

fn require_send_permission(
    connection: &Connection,
    actor: &PrincipalId,
    agent: agent_owner::AgentKey<'_>,
    actor_user: &str,
    item: &StoredItem,
) -> Result<()> {
    let permission = match item.intent {
        AgentInputIntent::Followup { .. } => agent_owner::AgentPermission::Submit,
        AgentInputIntent::Steer { .. } | AgentInputIntent::Interrupt { .. } => {
            agent_owner::AgentPermission::Control
        }
    };
    agent_owner::authorize_permission(connection, actor, agent, permission)?;
    if item.creator != actor_user {
        agent_owner::authorize_permission(
            connection,
            actor,
            agent,
            agent_owner::AgentPermission::Manage,
        )?;
    }
    Ok(())
}

fn selected_items(items: &[StoredItem], ids: Option<&[String]>) -> Result<Vec<StoredItem>> {
    let Some(ids) = ids else {
        if items.is_empty() {
            return Err(Error::Denied("Agent input Send selected no staged items"));
        }
        return Ok(items.to_vec());
    };
    if ids.is_empty() || ids.len() > MAX_ITEMS {
        return Err(Error::Limit("invalid staged Agent input Send count"));
    }
    let wanted = ids.iter().collect::<BTreeSet<_>>();
    if wanted.len() != ids.len() {
        return Err(Error::Denied("duplicate staged Agent input Send id"));
    }
    let selected = items
        .iter()
        .filter(|item| wanted.contains(&item.id))
        .cloned()
        .collect::<Vec<_>>();
    if selected.len() != wanted.len() {
        return Err(Error::Denied("staged Agent input Send item does not exist"));
    }
    Ok(selected)
}

fn runtime_request_id(request_id: &str, canonical_ref: &str, input_id: &str, kind: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-agent-input-runtime-v1\0");
    for part in [
        request_id.as_bytes(),
        canonical_ref.as_bytes(),
        input_id.as_bytes(),
        kind.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    format!("hi_{:x}", hash.finalize())
}

fn sender_token(actor_user: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"aw-agent-input-host-sender-v1\0");
    hash.update(actor_user.as_bytes());
    format!("host:{:x}", hash.finalize())
}

fn send_result_json(selected: &[StoredItem]) -> Result<String> {
    let items = selected
        .iter()
        .map(|item| {
            serde_json::json!({
                "id":item.id,
                "intent":item.intent,
                "creator":item.creator,
                "last_editor":item.last_editor,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({"sent":items}))
        .map_err(|_| Error::Storage("cannot encode staged Agent input Send result"))
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
        self.agent_inputs_add_inner(actor, canonical_ref, additions, expected_rev, None)
    }

    pub fn agent_inputs_add_host(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        request_id: &str,
        additions: &[AgentInputAdd],
        expected_rev: Option<u64>,
    ) -> Result<AgentInputPlanSnapshot> {
        validate_request_id(request_id)?;
        self.agent_inputs_add_inner(
            actor,
            canonical_ref,
            additions,
            expected_rev,
            Some(request_id),
        )
    }

    fn agent_inputs_add_inner(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        additions: &[AgentInputAdd],
        expected_rev: Option<u64>,
        request_id: Option<&str>,
    ) -> Result<AgentInputPlanSnapshot> {
        if additions.is_empty() || additions.len() > MAX_ITEMS {
            return Err(Error::Limit("invalid staged Agent input add count"));
        }
        let actor_user = self.current_user(actor)?.user_id;
        let fingerprint = request_id
            .map(|_| {
                mutation_fingerprint(
                    "add",
                    canonical_ref,
                    serde_json::json!({"items":additions,"rev":expected_rev}),
                )
            })
            .transpose()?;
        if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
            let connection = self.connection()?;
            let (session, branch, _) = agent_key(
                &connection,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            if let Some(state) = replay_state_in(
                &connection,
                &actor_user,
                request_id,
                "add",
                fingerprint,
                &session,
                &branch,
            )? {
                return match state {
                    MutationState::Applied => {
                        drop(connection);
                        self.agent_inputs_list(actor, canonical_ref)
                    }
                    MutationState::Prepared | MutationState::Unknown | MutationState::Rejected => {
                        Err(Error::Conflict(
                            "staged Agent input add request is non-retryable; inspect its mutation receipt",
                        ))
                    }
                };
            }
        }

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
            if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
                if replay_state_in(
                    &tx,
                    &actor_user,
                    request_id,
                    "add",
                    fingerprint,
                    &session,
                    &branch,
                )?
                .is_some()
                {
                    return Err(Error::Conflict(
                        "staged Agent input add request was claimed concurrently",
                    ));
                }
            }
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
            if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
                let result_json =
                    serde_json::to_string(&serde_json::json!({"ids":ids,"inputs_rev":next}))
                        .map_err(|_| Error::Storage("cannot encode staged input add result"))?;
                insert_receipt_in(
                    &tx,
                    &MutationReceipt {
                        actor_user: actor_user.clone(),
                        request_id: request_id.to_owned(),
                        operation: "add".into(),
                        fingerprint: fingerprint.to_owned(),
                        session: session.clone(),
                        branch: branch.clone(),
                        state: MutationState::Applied,
                        resulting_inputs_rev: Some(next),
                        result_json,
                    },
                    None,
                )?;
            }
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
        self.agent_inputs_edit_inner(actor, canonical_ref, id, edit, expected_rev, None)
    }

    pub fn agent_inputs_edit_host(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        request_id: &str,
        id: &str,
        edit: AgentInputEdit,
        expected_rev: u64,
    ) -> Result<AgentInputPlanSnapshot> {
        validate_request_id(request_id)?;
        self.agent_inputs_edit_inner(
            actor,
            canonical_ref,
            id,
            edit,
            expected_rev,
            Some(request_id),
        )
    }

    fn agent_inputs_edit_inner(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        id: &str,
        edit: AgentInputEdit,
        expected_rev: u64,
        request_id: Option<&str>,
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
        let fingerprint = request_id
            .map(|_| {
                mutation_fingerprint(
                    "edit",
                    canonical_ref,
                    serde_json::json!({"id":id,"edit":edit,"rev":expected_rev}),
                )
            })
            .transpose()?;
        if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
            let connection = self.connection()?;
            let (session, branch, _) = agent_key(
                &connection,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            if let Some(state) = replay_state_in(
                &connection,
                &actor_user,
                request_id,
                "edit",
                fingerprint,
                &session,
                &branch,
            )? {
                return match state {
                    MutationState::Applied => {
                        drop(connection);
                        self.agent_inputs_list(actor, canonical_ref)
                    }
                    MutationState::Prepared | MutationState::Unknown | MutationState::Rejected => {
                        Err(Error::Conflict(
                            "staged Agent input edit request is non-retryable; inspect its mutation receipt",
                        ))
                    }
                };
            }
        }

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
            if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
                if replay_state_in(
                    &tx,
                    &actor_user,
                    request_id,
                    "edit",
                    fingerprint,
                    &session,
                    &branch,
                )?
                .is_some()
                {
                    return Err(Error::Conflict(
                        "staged Agent input edit request was claimed concurrently",
                    ));
                }
            }
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
            if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
                let result_json =
                    serde_json::to_string(&serde_json::json!({"id":id,"inputs_rev":next}))
                        .map_err(|_| Error::Storage("cannot encode staged input edit result"))?;
                insert_receipt_in(
                    &tx,
                    &MutationReceipt {
                        actor_user: actor_user.clone(),
                        request_id: request_id.to_owned(),
                        operation: "edit".into(),
                        fingerprint: fingerprint.to_owned(),
                        session: session.clone(),
                        branch: branch.clone(),
                        state: MutationState::Applied,
                        resulting_inputs_rev: Some(next),
                        result_json,
                    },
                    None,
                )?;
            }
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
        self.agent_inputs_delete_inner(actor, canonical_ref, ids, expected_rev, None)
    }

    pub fn agent_inputs_delete_host(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        request_id: &str,
        ids: &[String],
        expected_rev: u64,
    ) -> Result<AgentInputPlanSnapshot> {
        validate_request_id(request_id)?;
        self.agent_inputs_delete_inner(actor, canonical_ref, ids, expected_rev, Some(request_id))
    }

    fn agent_inputs_delete_inner(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        ids: &[String],
        expected_rev: u64,
        request_id: Option<&str>,
    ) -> Result<AgentInputPlanSnapshot> {
        if ids.is_empty() || ids.len() > MAX_ITEMS {
            return Err(Error::Limit("invalid staged Agent input delete count"));
        }
        let unique = ids.iter().collect::<BTreeSet<_>>();
        if unique.len() != ids.len() {
            return Err(Error::Denied("duplicate staged Agent input id"));
        }
        let actor_user = self.current_user(actor)?.user_id;
        let fingerprint = request_id
            .map(|_| {
                let mut normalized = ids.to_vec();
                normalized.sort();
                mutation_fingerprint(
                    "delete",
                    canonical_ref,
                    serde_json::json!({"ids":normalized,"rev":expected_rev}),
                )
            })
            .transpose()?;
        if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
            let connection = self.connection()?;
            let (session, branch, _) = agent_key(
                &connection,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            if let Some(state) = replay_state_in(
                &connection,
                &actor_user,
                request_id,
                "delete",
                fingerprint,
                &session,
                &branch,
            )? {
                return match state {
                    MutationState::Applied => {
                        drop(connection);
                        self.agent_inputs_list(actor, canonical_ref)
                    }
                    MutationState::Prepared | MutationState::Unknown | MutationState::Rejected => {
                        Err(Error::Conflict(
                            "staged Agent input delete request is non-retryable; inspect its mutation receipt",
                        ))
                    }
                };
            }
        }

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
            if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
                if replay_state_in(
                    &tx,
                    &actor_user,
                    request_id,
                    "delete",
                    fingerprint,
                    &session,
                    &branch,
                )?
                .is_some()
                {
                    return Err(Error::Conflict(
                        "staged Agent input delete request was claimed concurrently",
                    ));
                }
            }
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
            if let (Some(request_id), Some(fingerprint)) = (request_id, fingerprint.as_deref()) {
                let result_json =
                    serde_json::to_string(&serde_json::json!({"ids":ids,"inputs_rev":next}))
                        .map_err(|_| Error::Storage("cannot encode staged input delete result"))?;
                insert_receipt_in(
                    &tx,
                    &MutationReceipt {
                        actor_user: actor_user.clone(),
                        request_id: request_id.to_owned(),
                        operation: "delete".into(),
                        fingerprint: fingerprint.to_owned(),
                        session: session.clone(),
                        branch: branch.clone(),
                        state: MutationState::Applied,
                        resulting_inputs_rev: Some(next),
                        result_json,
                    },
                    None,
                )?;
            }
            db(tx.commit())?;
        }
        for prepared in &removed {
            let _ = self.discard_prepared_queue_custody(prepared);
        }
        self.agent_inputs_list(actor, canonical_ref)
    }

    pub fn agent_inputs_prepare_send(
        &self,
        actor: &PrincipalId,
        canonical_ref: &str,
        request_id: &str,
        ids: Option<&[String]>,
        expected_rev: u64,
    ) -> Result<AgentInputSendStart> {
        validate_request_id(request_id)?;
        let actor_user = self.current_user(actor)?.user_id;
        let semantic_ids = ids.map(|ids| {
            let mut normalized = ids.to_vec();
            normalized.sort();
            normalized
        });
        let fingerprint = mutation_fingerprint(
            "send",
            canonical_ref,
            serde_json::json!({"ids":semantic_ids,"rev":expected_rev}),
        )?;

        let prepared = {
            let connection = self.connection()?;
            let (session, branch, _) = agent_key(
                &connection,
                actor,
                canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            if let Some(receipt) = receipt_in(&connection, &actor_user, request_id)? {
                validate_replay(&receipt, "send", &fingerprint, &session, &branch)?;
                return match receipt.state {
                    MutationState::Applied => {
                        drop(connection);
                        Ok(AgentInputSendStart::Replay(
                            self.agent_inputs_list(actor, canonical_ref)?,
                        ))
                    }
                    MutationState::Prepared | MutationState::Unknown | MutationState::Rejected => {
                        Err(Error::Conflict(
                            "Agent input Send request is non-retryable; inspect its mutation receipt",
                        ))
                    }
                };
            }
            require_open(&connection, &session, &branch)?;
            let current = plan_rev(&connection, &session, &branch)?;
            require_rev(current, Some(expected_rev))?;
            let items = load_items(&connection, &session, &branch)?;
            let selected = selected_items(&items, ids)?;
            let agent = agent_owner::AgentKey::new(&session, &branch);
            let mut control_index = None;
            for (index, item) in selected.iter().enumerate() {
                require_send_permission(&connection, actor, agent, &actor_user, item)?;
                validate_intent(&connection, &session, &branch, &item.intent)?;
                if !matches!(item.intent, AgentInputIntent::Followup { .. })
                    && control_index.replace(index).is_some()
                {
                    return Err(Error::Conflict(
                        "one Agent input Send may contain at most one steer or interrupt",
                    ));
                }
            }
            (session, branch, selected, control_index)
        };

        let vault = self.local_secret_store()?;
        let mut control = None;
        for (index, item) in prepared.2.iter().enumerate() {
            let bytes = vault.get(&item.payload_ref)?;
            if format!("{:x}", Sha256::digest(bytes.as_slice())) != item.payload_sha256 {
                return Err(Error::Storage(
                    "staged Agent input payload digest no longer matches",
                ));
            }
            if prepared.3 == Some(index) {
                let text = String::from_utf8(bytes.to_vec())
                    .map_err(|_| Error::Storage("staged Agent input payload is not UTF-8"))?;
                let (turn, boundary, kind) = match item.intent {
                    AgentInputIntent::Steer { turn } => {
                        (turn, aw_agent_core::AuthorityBoundaryKind::Steer, "steer")
                    }
                    AgentInputIntent::Interrupt { turn } => (
                        turn,
                        aw_agent_core::AuthorityBoundaryKind::Interrupt,
                        "interrupt",
                    ),
                    AgentInputIntent::Followup { .. } => unreachable!("selected control index"),
                };
                control = Some(AgentInputSendControl {
                    request_id: runtime_request_id(request_id, canonical_ref, &item.id, kind),
                    turn,
                    boundary,
                    text: Zeroizing::new(text),
                });
            }
        }

        Ok(AgentInputSendStart::Prepared(Box::new(
            PreparedAgentInputSend {
                canonical_ref: canonical_ref.to_owned(),
                session: prepared.0,
                branch: prepared.1,
                actor_user,
                request_id: request_id.to_owned(),
                fingerprint,
                expected_rev,
                selected: prepared.2,
                control,
                runtime_owner: None,
                recipient_site: None,
                writer_epoch: None,
                prepared_control: None,
            },
        )))
    }

    pub fn agent_inputs_bind_send_runtime(
        &self,
        actor: &PrincipalId,
        mut prepared: PreparedAgentInputSend,
        runtime_owner: PrincipalId,
        recipient_site: &str,
        writer_epoch: u64,
    ) -> Result<PreparedAgentInputSend> {
        if recipient_site.is_empty()
            || recipient_site.len() > 128
            || !recipient_site.is_ascii()
            || recipient_site.chars().any(char::is_control)
            || writer_epoch == 0
        {
            return Err(Error::Denied("invalid live Agent input owner binding"));
        }
        {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (session, branch, _) = agent_key(
                &tx,
                actor,
                &prepared.canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            if session != prepared.session || branch != prepared.branch {
                return Err(Error::Conflict(
                    "Agent input Send target changed before live binding",
                ));
            }
            require_open(&tx, &session, &branch)?;
            require_rev(
                plan_rev(&tx, &session, &branch)?,
                Some(prepared.expected_rev),
            )?;
            if !principal_is_session_owner(&tx, &session, &runtime_owner)? {
                return Err(Error::Denied(
                    "live Agent input owner differs from durable Session owner",
                ));
            }
            workspace_writer::verify_writer_epoch_for_session(&tx, &session, writer_epoch)?;
            let current = load_items(&tx, &session, &branch)?;
            let ids = prepared
                .selected
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            if selected_items(&current, Some(&ids))? != prepared.selected {
                return Err(Error::Conflict(
                    "staged Agent input selection changed before live binding",
                ));
            }
            if prepared.control.is_some() {
                if receipt_in(&tx, &prepared.actor_user, &prepared.request_id)?.is_some() {
                    return Err(Error::Conflict(
                        "Agent input Send mutation identity was claimed concurrently",
                    ));
                }
                insert_receipt_in(
                    &tx,
                    &MutationReceipt {
                        actor_user: prepared.actor_user.clone(),
                        request_id: prepared.request_id.clone(),
                        operation: "send".into(),
                        fingerprint: prepared.fingerprint.clone(),
                        session: session.clone(),
                        branch: branch.clone(),
                        state: MutationState::Prepared,
                        resulting_inputs_rev: None,
                        result_json: send_result_json(&prepared.selected)?,
                    },
                    None,
                )?;
            }
            db(tx.commit())?;
        }

        prepared.runtime_owner = Some(runtime_owner.clone());
        prepared.recipient_site = Some(recipient_site.to_owned());
        prepared.writer_epoch = Some(writer_epoch);
        if let Some(control) = &prepared.control {
            let result = self.authority_steer_prepare(
                &runtime_owner,
                crate::AuthoritySteerRequest {
                    request_id: control.request_id.clone(),
                    session_id: prepared.session.clone(),
                    branch_id: prepared.branch.clone(),
                    writer_epoch,
                    turn_id: control.turn,
                    canonical_target: format!("{}/t:{}", prepared.canonical_ref, control.turn),
                    participant: None,
                },
                control.text(),
            );
            match result {
                Ok(fact) => prepared.prepared_control = Some(fact),
                Err(error) => {
                    let note = error.to_string().chars().take(1024).collect::<String>();
                    let mut connection = self.connection()?;
                    let tx =
                        db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
                    let receipt = receipt_in(&tx, &prepared.actor_user, &prepared.request_id)?
                        .ok_or(Error::Storage(
                            "Agent input Send mutation receipt disappeared",
                        ))?;
                    validate_replay(
                        &receipt,
                        "send",
                        &prepared.fingerprint,
                        &prepared.session,
                        &prepared.branch,
                    )?;
                    let _ = transition_receipt_in(
                        &tx,
                        &receipt,
                        MutationState::Rejected,
                        None,
                        &receipt.result_json,
                        Some(&note),
                    )?;
                    db(tx.commit())?;
                    return Err(error);
                }
            }
        }
        Ok(prepared)
    }

    pub fn agent_inputs_commit_send(
        &self,
        actor: &PrincipalId,
        prepared: &PreparedAgentInputSend,
        control_segment: Option<&aw_agent_core::AuthoritySegment>,
    ) -> Result<AgentInputPlanSnapshot> {
        let runtime_owner = prepared.runtime_owner.as_ref().ok_or(Error::Conflict(
            "Agent input Send lacks live runtime owner binding",
        ))?;
        let recipient_site = prepared.recipient_site.as_deref().ok_or(Error::Conflict(
            "Agent input Send lacks live recipient site",
        ))?;
        let writer_epoch = prepared
            .writer_epoch
            .ok_or(Error::Conflict("Agent input Send lacks live writer epoch"))?;
        if prepared.control.is_some() != control_segment.is_some()
            || prepared.control.is_some() != prepared.prepared_control.is_some()
        {
            return Err(Error::Conflict(
                "Agent input Send control preparation is incomplete",
            ));
        }

        let control_staged_custody = prepared.control.as_ref().and_then(|control| {
            prepared
                .selected
                .iter()
                .find(|item| {
                    matches!(
                        (&item.intent, control.boundary),
                        (
                            AgentInputIntent::Steer { turn },
                            aw_agent_core::AuthorityBoundaryKind::Steer
                        ) if *turn == control.turn
                    ) || matches!(
                        (&item.intent, control.boundary),
                        (
                            AgentInputIntent::Interrupt { turn },
                            aw_agent_core::AuthorityBoundaryKind::Interrupt
                        ) if *turn == control.turn
                    )
                })
                .map(|item| actor_queue::PreparedQueueCustody {
                    payload_ref: item.payload_ref.clone(),
                    payload_sha256: item.payload_sha256.clone(),
                })
        });

        {
            let mut connection = self.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (session, branch, _) = agent_key(
                &tx,
                actor,
                &prepared.canonical_ref,
                agent_owner::AgentPermission::View,
            )?;
            if session != prepared.session || branch != prepared.branch {
                return Err(Error::Conflict(
                    "Agent input Send target changed before publication",
                ));
            }
            require_open(&tx, &session, &branch)?;
            let current_rev = plan_rev(&tx, &session, &branch)?;
            require_rev(current_rev, Some(prepared.expected_rev))?;
            if !principal_is_session_owner(&tx, &session, runtime_owner)? {
                return Err(Error::Denied(
                    "live Agent input owner differs from durable Session owner",
                ));
            }
            workspace_writer::verify_writer_epoch_for_session(&tx, &session, writer_epoch)?;
            let current = load_items(&tx, &session, &branch)?;
            let ids = prepared
                .selected
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            let selected = selected_items(&current, Some(&ids))?;
            if selected != prepared.selected {
                return Err(Error::Conflict(
                    "staged Agent input selection changed before Send publication",
                ));
            }
            let agent = agent_owner::AgentKey::new(&session, &branch);
            for item in &selected {
                require_send_permission(&tx, actor, agent, &prepared.actor_user, item)?;
                validate_intent(&tx, &session, &branch, &item.intent)?;
            }

            if let (Some(fact), Some(segment)) =
                (prepared.prepared_control.as_ref(), control_segment)
            {
                let receipt = receipt_in(&tx, &prepared.actor_user, &prepared.request_id)?.ok_or(
                    Error::Storage("Agent input Send mutation receipt disappeared"),
                )?;
                validate_replay(&receipt, "send", &prepared.fingerprint, &session, &branch)?;
                if receipt.state != MutationState::Prepared {
                    return Err(Error::Conflict(
                        "Agent input Send mutation receipt is not Prepared",
                    ));
                }
                self.authority_steer_accept_in(&tx, runtime_owner, fact, segment)?;
            } else if receipt_in(&tx, &prepared.actor_user, &prepared.request_id)?.is_some() {
                return Err(Error::Conflict(
                    "follow-up Agent input Send mutation identity was claimed concurrently",
                ));
            }

            let sender = sender_token(&prepared.actor_user);
            let mut message_id = actor_queue::next_message_id_in(&tx, &session, &sender)?;
            for item in &selected {
                if !matches!(item.intent, AgentInputIntent::Followup { .. }) {
                    continue;
                }
                actor_queue::publish_prepared_queue_in(
                    &tx,
                    runtime_owner,
                    crate::ActorQueueRequest {
                        request_id: runtime_request_id(
                            &prepared.request_id,
                            &prepared.canonical_ref,
                            &item.id,
                            "followup",
                        ),
                        session_id: session.clone(),
                        branch_id: branch.clone(),
                        recipient_site: recipient_site.to_owned(),
                        sender: sender.clone(),
                        message_id,
                        writer_epoch,
                        target: crate::QueueTarget::Auto,
                        priority: 0,
                    },
                    &actor_queue::PreparedQueueCustody {
                        payload_ref: item.payload_ref.clone(),
                        payload_sha256: item.payload_sha256.clone(),
                    },
                )?;
                message_id = message_id
                    .checked_add(1)
                    .ok_or(Error::Limit("Actor queue sender sequence exhausted"))?;
            }

            for item in &selected {
                db(tx.execute(
                    "DELETE FROM agent_input_plan_items
                     WHERE session_id=?1 AND branch_id=?2 AND input_id=?3",
                    params![session, branch, item.id],
                ))?;
            }
            let next = next_rev(&tx, &session, &branch, current_rev)?;
            agent_owner::changed(&tx, agent)?;
            publish_changed(&tx, actor, &session, &branch, next)?;
            let result_json = send_result_json(&selected)?;
            if let Some(receipt) = receipt_in(&tx, &prepared.actor_user, &prepared.request_id)? {
                let _ = transition_receipt_in(
                    &tx,
                    &receipt,
                    MutationState::Applied,
                    Some(next),
                    &result_json,
                    None,
                )?;
            } else {
                insert_receipt_in(
                    &tx,
                    &MutationReceipt {
                        actor_user: prepared.actor_user.clone(),
                        request_id: prepared.request_id.clone(),
                        operation: "send".into(),
                        fingerprint: prepared.fingerprint.clone(),
                        session: session.clone(),
                        branch: branch.clone(),
                        state: MutationState::Applied,
                        resulting_inputs_rev: Some(next),
                        result_json,
                    },
                    None,
                )?;
            }
            db(tx.commit())?;
        }

        if let Some(custody) = control_staged_custody {
            let _ = self.discard_prepared_queue_custody(&custody);
        }
        self.agent_inputs_list(actor, &prepared.canonical_ref)
    }

    pub fn agent_inputs_abort_send(
        &self,
        prepared: &PreparedAgentInputSend,
        uncertain: bool,
        note: &str,
    ) -> Result<()> {
        let Some(fact) = prepared.prepared_control.as_ref() else {
            return Ok(());
        };
        if note.trim().is_empty() {
            return Err(Error::Denied("Agent input Send abort note is empty"));
        }
        let runtime_owner = prepared.runtime_owner.as_ref().ok_or(Error::Conflict(
            "Agent input Send abort lacks runtime owner",
        ))?;
        let bounded = note.chars().take(1024).collect::<String>();
        let authority = if uncertain {
            self.authority_steer_mark_unknown(runtime_owner, fact, &bounded)
        } else {
            self.authority_steer_reject_prepared(runtime_owner, fact, &bounded)
        };
        let state = if uncertain || authority.is_err() {
            MutationState::Unknown
        } else {
            MutationState::Rejected
        };
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let receipt = receipt_in(&tx, &prepared.actor_user, &prepared.request_id)?.ok_or(
            Error::Storage("Agent input Send mutation receipt disappeared"),
        )?;
        validate_replay(
            &receipt,
            "send",
            &prepared.fingerprint,
            &prepared.session,
            &prepared.branch,
        )?;
        if receipt.state == MutationState::Prepared {
            let result_json = receipt.result_json.clone();
            let _ =
                transition_receipt_in(&tx, &receipt, state, None, &result_json, Some(&bounded))?;
        }
        db(tx.commit())?;
        Ok(())
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
    fn v43_staged_input_plan_is_revisioned_encrypted_replay_safe_and_frozen_on_close() {
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
                    "DROP TABLE agent_input_mutation_replay;
                     DROP TABLE agent_input_plan_items;
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
            crate::migrate_agent_input_replay(&mut connection).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                43
            );
        }

        let initial_agent_rev = store.agent_get(&actor, &canonical).unwrap().rev;
        let empty = store.agent_inputs_list(&actor, &canonical).unwrap();
        assert_eq!(empty.inputs_rev, 1);
        assert!(empty.items.is_empty());

        let first_item = AgentInputAdd {
            text: "first".into(),
            intent: AgentInputIntent::Followup { after: None },
            before: None,
            after: None,
        };
        let first = store
            .agent_inputs_add_host(
                &actor,
                &canonical,
                "input-add-1",
                std::slice::from_ref(&first_item),
                None,
            )
            .unwrap();
        let first_replay = store
            .agent_inputs_add_host(
                &actor,
                &canonical,
                "input-add-1",
                std::slice::from_ref(&first_item),
                None,
            )
            .unwrap();
        assert_eq!(first_replay.inputs_rev, first.inputs_rev);
        assert_eq!(first_replay.items.len(), 1);
        let mut changed_first = first_item.clone();
        changed_first.text = "changed replay".into();
        assert!(matches!(
            store.agent_inputs_add_host(&actor, &canonical, "input-add-1", &[changed_first], None,),
            Err(Error::Conflict(_))
        ));
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

        let edit = AgentInputEdit {
            text: Some("first edited".into()),
            intent: None,
            before: Some(second_id.clone()),
            after: None,
        };
        let edited = store
            .agent_inputs_edit_host(
                &actor,
                &canonical,
                "input-edit-1",
                &first_id,
                edit.clone(),
                second.inputs_rev,
            )
            .unwrap();
        let edited_replay = store
            .agent_inputs_edit_host(
                &actor,
                &canonical,
                "input-edit-1",
                &first_id,
                edit,
                second.inputs_rev,
            )
            .unwrap();
        assert_eq!(edited_replay.inputs_rev, edited.inputs_rev);
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
            .agent_inputs_delete_host(
                &actor,
                &canonical,
                "input-delete-1",
                std::slice::from_ref(&second_id),
                edited.inputs_rev,
            )
            .unwrap();
        let deleted_replay = store
            .agent_inputs_delete_host(
                &actor,
                &canonical,
                "input-delete-1",
                std::slice::from_ref(&second_id),
                edited.inputs_rev,
            )
            .unwrap();
        assert_eq!(deleted_replay.inputs_rev, deleted.inputs_rev);
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
