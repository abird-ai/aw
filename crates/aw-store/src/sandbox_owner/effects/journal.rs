//! Link source custody to one original direct ToolStarted/ToolFinished pair.
//! The journal owns the effect; the commit log records only custody/release.
use super::{SandboxEffectOutcome, SandboxEffectPlan, SandboxEffectRequest};
use crate::{
    agent_owner::{self, AgentKey, AgentPermission},
    db, durability, label,
    store_scope::StoreScope,
    EffectiveSourceBinding, Error, Result, SessionStore,
};
use aw_agent_core::{
    tool::ToolBindingSnapshot, DriverKey, InvocationIdentity, JournalEntry, NamedSourceAccess,
    Outcome, PrincipalId, SourceAccess, SourceSetLimit, ToolEffect, ToolRecord, ToolSourceEvidence,
    ToolStatus, Turn,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const EFFECT_PREFIX: &str = "source-tool:";
const ENTITY: &str = "source_effect";
const CLAIMED: &str = "source.effect.claimed";
const RELEASED: &str = "source.effect.released";
const QUIESCENT: &str = "source.effect.quiescent";

/// Original journal identity, not a capability or a new attempt ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Intent {
    pub(super) agent_ref: String,
    pub(super) turn_id: u64,
    pub(super) call_id: String,
    pub(super) run_id: String,
    pub(super) started_seq: u64,
    pub(super) actor: PrincipalId,
}

impl Intent {
    pub(super) fn effect_id(&self) -> String {
        format!("{EFFECT_PREFIX}{}", self.started_seq)
    }
}

struct Observation {
    intent: Intent,
    record: ToolRecord,
    source: ToolSourceEvidence,
    terminal: Option<(u64, ToolRecord)>,
    active: bool,
    writer_epoch: u64,
}

fn observe(c: &Connection, agent_ref: &str, turn_id: u64, call_id: &str) -> Result<Observation> {
    label(agent_ref, 2048)?;
    label(call_id, 128)?;
    if turn_id == 0 {
        return Err(Error::Config(
            "source effect requires an original Turn".into(),
        ));
    }
    let (session, branch) = agent_owner::key_for_ref(c, agent_ref)?;
    let (run_id, identity, active, writer_epoch): (String, String, bool, u64) = db(c
        .query_row(
            "SELECT r.id,r.identity_json,r.status='running' AND b.active_run IS r.id,
                    r.produced_from_writer_epoch
         FROM runs r JOIN branches b ON b.session_id=r.session_id AND b.id=r.branch_id
         WHERE r.session_id=?1 AND r.branch_id=?2 AND r.turn_id=?3",
            params![session, branch, turn_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional())?
    .ok_or(Error::Denied("original source effect Turn unavailable"))?;
    let mut query = db(c.prepare_cached(
        "SELECT seq,kind,payload FROM run_events
         WHERE run_id=?1 AND kind IN ('tool_started','tool_finished','tool_denied')
           AND json_extract(payload,'$.record.call.id')=?2 ORDER BY seq LIMIT 3",
    ))?;
    let mut rows = db(query.query(params![run_id, call_id]))?;
    let start = db(rows.next())?.ok_or(Error::Conflict("original ToolStarted is missing"))?;
    let started_seq: u64 = db(start.get(0))?;
    let kind: String = db(start.get(1))?;
    let JournalEntry::ToolStarted {
        record,
        source: Some(source),
    } = crate::decode::<JournalEntry>(&db(start.get::<_, String>(2))?)?
    else {
        return Err(Error::Conflict(
            "original ToolStarted lacks exact source provenance",
        ));
    };
    record.call.validate_envelope()?;
    source.validate()?;
    if kind != "tool_started"
        || record.call.id != call_id
        || record.status != ToolStatus::Running
        || record.output.is_some()
        || !matches!(
            record.effect,
            ToolEffect::Read | ToolEffect::Write | ToolEffect::Process
        )
    {
        return Err(Error::Conflict("invalid original source effect intent"));
    }
    let terminal = if let Some(row) = db(rows.next())? {
        let seq: u64 = db(row.get(0))?;
        let kind: String = db(row.get(1))?;
        let JournalEntry::ToolFinished { record: finished } =
            crate::decode::<JournalEntry>(&db(row.get::<_, String>(2))?)?
        else {
            return Err(Error::Conflict(
                "original source effect journal is ambiguous",
            ));
        };
        if kind != "tool_finished"
            || seq <= started_seq
            || finished.call != record.call
            || finished.effect != record.effect
            || finished.execution_site != record.execution_site
            || matches!(finished.status, ToolStatus::Running | ToolStatus::Denied)
            || db(rows.next())?.is_some()
        {
            return Err(Error::Conflict(
                "original ToolFinished does not match its unique intent",
            ));
        }
        Some((seq, finished))
    } else {
        None
    };
    // The original authority segment at start, not the current Agent ACL.
    // The original owner Input has no participant evidence. Only its
    // exact historical Actor dispatch may corroborate the Run identity.
    let segment: Option<String> = db(c
        .query_row(
            "SELECT payload FROM run_events WHERE run_id=?1 AND seq<?2
           AND kind='authority_segment_started' ORDER BY seq DESC LIMIT 1",
            params![run_id, started_seq],
            |row| row.get(0),
        )
        .optional())?;
    let actor = if let Some(segment) = segment {
        let JournalEntry::AuthoritySegmentStarted { segment } = crate::decode(&segment)? else {
            return Err(Error::Storage("invalid original AuthoritySegment"));
        };
        segment.validate()?;
        match segment.evidence {
            Some(evidence) => evidence.actor,
            None if segment.ordinal == 1
                && segment.boundary == aw_agent_core::AuthorityBoundaryKind::Input =>
            {
                let (head, revision, prompt_node): (String, u64, String) = db(c.query_row(
                    "SELECT produced_from_head,produced_from_revision,prompt_node FROM runs WHERE id=?1",
                    [&run_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                ))?;
                let prompt = crate::node_in(c, &session, &prompt_node)?;
                let actor = crate::decode::<InvocationIdentity>(&identity)?.actor;
                let input = crate::actor_admission::source_dispatch_in(
                    c,
                    &session,
                    &branch,
                    &head,
                    revision,
                    turn_id,
                    writer_epoch,
                    prompt.payload["prompt"]
                        .as_str()
                        .ok_or(Error::Storage("source Run prompt unavailable"))?,
                    false,
                )?;
                if prompt.actor != actor
                    || segment.boundary_id
                        != format!("input:{}:{}", input.message_id, input.message_id)
                {
                    return Err(Error::Denied(
                        "source Run differs from its original owner Input segment",
                    ));
                }
                actor
            }
            None => {
                return Err(Error::Denied(
                    "original authority segment actor unavailable",
                ))
            }
        }
    } else {
        crate::decode::<InvocationIdentity>(&identity)?.actor
    };
    Ok(Observation {
        intent: Intent {
            agent_ref: agent_ref.to_owned(),
            turn_id,
            call_id: call_id.to_owned(),
            run_id,
            started_seq,
            actor,
        },
        record,
        source,
        terminal,
        active,
        writer_epoch,
    })
}

pub(super) fn admit_in(
    tx: &Transaction<'_>,
    request: &SandboxEffectRequest,
    turn_id: u64,
    call: &aw_agent_core::ToolCall,
    binding: &ToolBindingSnapshot,
    admitted: Option<&Intent>,
) -> Result<Intent> {
    let observed = observe(tx, &request.agent_ref, turn_id, &call.id)?;
    if !observed.active
        || observed.terminal.is_some()
        || observed.intent.actor != request.actor
        || &observed.record.call != call
        || observed.record.call.name != binding.name
        || observed.record.effect != binding.effect
        || observed.record.execution_site != binding.execution_site
        || observed.source != ToolSourceEvidence::from_source(&binding.source)
        || (binding.effect == ToolEffect::Read
            && request
                .sources
                .iter()
                .any(|source| source.access == SourceAccess::ReadWrite))
    {
        return Err(Error::Denied(
            "source effect differs from its active original tool intent",
        ));
    }
    let (session, _) = agent_owner::key_for_ref(tx, &request.agent_ref)?;
    crate::workspace_writer::verify_writer_epoch_for_session(tx, &session, observed.writer_epoch)?;
    if let Some(admitted) = admitted {
        if admitted != &observed.intent {
            return Err(Error::Conflict("original admitted tool intent changed"));
        }
        require_claim(tx, admitted)?;
        if fact_in::<Release>(tx, &admitted.effect_id(), RELEASED)?.is_some()
            || fact_in::<SandboxEffectOutcome>(tx, &admitted.effect_id(), QUIESCENT)?.is_some()
        {
            return Err(Error::Conflict(
                "original source effect is no longer launchable",
            ));
        }
    } else if claim_in(tx, &observed.intent.effect_id())?.is_some() {
        return Err(Error::Conflict(
            "original source effect was already claimed; never relaunch",
        ));
    }
    Ok(observed.intent)
}

/// One immutable custody link. Retain the named modes and digest of the
/// complete versioned bindings, not a second copy of source state or ToolRecord.
/// The original barrier prevents participating structural mutation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    intent: Intent,
    sandbox_ref: String,
    store_generation: u64,
    driver: DriverKey,
    sources: Vec<NamedSourceAccess>,
    bindings_digest: [u8; 32],
}

fn bindings_digest(sources: &[EffectiveSourceBinding]) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    hash.update(b"aw-source-effect-bindings-v1\0");
    for source in sources {
        let encoded = crate::json(&(&source.binding, source.access))?;
        hash.update((encoded.len() as u64).to_be_bytes());
        hash.update(encoded.as_bytes());
    }
    Ok(hash.finalize().into())
}

fn fact_in<T: serde::de::DeserializeOwned>(
    c: &Connection,
    effect_id: &str,
    kind: &str,
) -> Result<Option<T>> {
    let mut query = db(c.prepare_cached(
        "SELECT store_id,payload_json FROM commit_log
         WHERE entity_type='source_effect' AND entity_id=?1 AND kind=?2 LIMIT 2",
    ))?;
    let mut rows = db(query.query(params![effect_id, kind]))?;
    let Some(row) = db(rows.next())? else {
        return Ok(None);
    };
    if db(row.get::<_, String>(0))? != StoreScope::load(c)?.id().as_str() {
        return Err(Error::Conflict("source effect belongs to another Store"));
    }
    let encoded: String = db(row.get(1))?;
    if db(rows.next())?.is_some() {
        return Err(Error::Storage("duplicate source effect custody fact"));
    }
    #[derive(Deserialize)]
    struct Payload<T> {
        sync_scope: String,
        fact: T,
    }
    let payload: Payload<T> = crate::decode(&encoded)?;
    if payload.sync_scope != "local" {
        return Err(Error::Storage("source effect custody must be local"));
    }
    Ok(Some(payload.fact))
}

fn claim_in(c: &Connection, effect_id: &str) -> Result<Option<Claim>> {
    let claim: Option<Claim> = fact_in(c, effect_id, CLAIMED)?;
    if let Some(claim) = &claim {
        if claim.intent.effect_id() != effect_id {
            return Err(Error::Storage("source effect custody identity changed"));
        }
    }
    Ok(claim)
}

fn append_fact(
    tx: &Transaction<'_>,
    effect_id: &str,
    kind: &str,
    actor: &PrincipalId,
    fact: &impl Serialize,
) -> Result<()> {
    durability::commit_event(
        tx,
        &durability::tx_id()?,
        kind,
        ENTITY,
        effect_id,
        Some(actor),
        serde_json::json!({"sync_scope":"local","fact":fact}),
    )
}

pub(super) fn claim_effect_in(
    tx: &Transaction<'_>,
    intent: &Intent,
    plan: &SandboxEffectPlan,
    scope: &StoreScope,
) -> Result<()> {
    if plan.effect_id != intent.effect_id()
        || plan.grant.agent_ref != intent.agent_ref
        || plan.grant.actor != intent.actor
        || claim_in(tx, &plan.effect_id)?.is_some()
    {
        return Err(Error::Conflict("original source effect custody changed"));
    }
    let claim = Claim {
        intent: intent.clone(),
        sandbox_ref: plan.grant.sandbox_ref.clone(),
        store_generation: scope.restore_generation(),
        driver: plan.driver.driver.clone(),
        sources: plan
            .grant
            .sources
            .iter()
            .map(|source| NamedSourceAccess {
                name: source.binding.binding.name.clone(),
                access: source.access,
            })
            .collect(),
        bindings_digest: bindings_digest(&plan.grant.sources)?,
    };
    append_fact(tx, &plan.effect_id, CLAIMED, &intent.actor, &claim)
}

fn require_claim(c: &Connection, intent: &Intent) -> Result<Claim> {
    let claim = claim_in(c, &intent.effect_id())?
        .ok_or(Error::Conflict("original tool has no source custody"))?;
    if claim.intent != *intent
        || claim.store_generation != StoreScope::load(c)?.restore_generation()
    {
        return Err(Error::Conflict("source effect custody boundary changed"));
    }
    Ok(claim)
}

/// The existing journal owns both forms of logical closure. An interrupted
/// Turn is not a fabricated ToolFinished or a known physical outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxToolTerminalRef {
    ToolFinished(u64),
    InterruptedTurn(String),
}

fn terminal_in(
    c: &Connection,
    observed: &Observation,
) -> Result<(SandboxToolTerminalRef, ToolRecord)> {
    if let Some((seq, record)) = &observed.terminal {
        return Ok((SandboxToolTerminalRef::ToolFinished(*seq), record.clone()));
    }
    // A missing ToolFinished can only be closed by the original Run owner.
    // Require its settled, immutable Turn and exact Interrupted projection.
    // Never interpret a missing active branch pointer alone as closure.
    let (session, prompt, node_id): (String, String, String) = db(c
        .query_row(
            "SELECT r.session_id,r.prompt_node,r.final_node FROM runs r
         JOIN branches b ON b.session_id=r.session_id AND b.id=r.branch_id
         WHERE r.id=?1 AND r.status IN ('failed','cancelled','incomplete','interrupted')
           AND r.ended_at IS NOT NULL AND r.final_node IS NOT NULL
           AND r.attach_status IN ('attached','conflict') AND b.active_run IS NOT r.id",
            [&observed.intent.run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional())?
    .ok_or(Error::Conflict(
        "original tool has no settled journal boundary",
    ))?;
    let node = crate::node_in(c, &session, &node_id)?;
    if node.kind != "turn" || node.parent_id.as_deref() != Some(prompt.as_str()) {
        return Err(Error::Storage("original settled Run Turn identity changed"));
    }
    let turn: Turn = serde_json::from_value(node.payload)
        .map_err(|_| Error::Storage("invalid original settled Run Turn"))?;
    if turn.id != observed.intent.turn_id
        || turn.outcome == Outcome::Completed
        || !turn.needs_reconciliation
    {
        return Err(Error::Conflict(
            "original Turn did not preserve interrupted effect",
        ));
    }
    let mut matches = turn
        .tool_records
        .into_iter()
        .filter(|record| record.call.id == observed.intent.call_id);
    let record = matches.next().ok_or(Error::Conflict(
        "original Turn lacks interrupted tool intent",
    ))?;
    if record.call != observed.record.call
        || record.effect != observed.record.effect
        || record.execution_site != observed.record.execution_site
        || record.status != ToolStatus::Interrupted
        || record.output.is_some()
        || matches.next().is_some()
    {
        return Err(Error::Conflict(
            "original Turn differs from interrupted tool intent",
        ));
    }
    Ok((SandboxToolTerminalRef::InterruptedTurn(node_id), record))
}

/// Retain only the original closure identity, never a second tool result.
/// Existing numeric ToolFinished release receipts keep their exact encoding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interrupted_turn: Option<String>,
    outcome: SandboxEffectOutcome,
}

impl Release {
    fn new(terminal: SandboxToolTerminalRef, outcome: SandboxEffectOutcome) -> Self {
        let (terminal_seq, interrupted_turn) = match terminal {
            SandboxToolTerminalRef::ToolFinished(seq) => (Some(seq), None),
            SandboxToolTerminalRef::InterruptedTurn(node) => (None, Some(node)),
        };
        Self {
            terminal_seq,
            interrupted_turn,
            outcome,
        }
    }
}

/// Positive physical settlement, separate from the logical ToolFinished.
/// No copy of ProcessTerminal, tool output or mutable operation state.
pub(super) fn record_quiescence_in(
    tx: &Transaction<'_>,
    intent: &Intent,
    outcome: SandboxEffectOutcome,
) -> Result<()> {
    require_claim(tx, intent)?;
    if outcome == SandboxEffectOutcome::Unknown
        || fact_in::<SandboxEffectOutcome>(tx, &intent.effect_id(), QUIESCENT)?.is_some()
        || fact_in::<Release>(tx, &intent.effect_id(), RELEASED)?.is_some()
    {
        return Err(Error::Conflict(
            "source effect already settled or not proven quiescent",
        ));
    }
    append_fact(tx, &intent.effect_id(), QUIESCENT, &intent.actor, &outcome)
}

pub(super) fn release_in(
    tx: &Transaction<'_>,
    intent: &Intent,
    actor: &PrincipalId,
    outcome: SandboxEffectOutcome,
) -> Result<()> {
    require_claim(tx, intent)?;
    let observed = observe(tx, &intent.agent_ref, intent.turn_id, &intent.call_id)?;
    if observed.intent != *intent {
        return Err(Error::Conflict("original source effect intent changed"));
    }
    let (terminal, record) = terminal_in(tx, &observed)?;
    if outcome == SandboxEffectOutcome::Unknown
        || fact_in::<SandboxEffectOutcome>(tx, &intent.effect_id(), QUIESCENT)?
            .is_some_and(|prior| prior != outcome)
        || (outcome == SandboxEffectOutcome::NotStarted && record.status == ToolStatus::Succeeded)
        || fact_in::<Release>(tx, &intent.effect_id(), RELEASED)?.is_some()
    {
        return Err(Error::Conflict(
            "invalid or already settled source custody release",
        ));
    }
    append_fact(
        tx,
        &intent.effect_id(),
        RELEASED,
        actor,
        &Release::new(terminal, outcome),
    )
}

/// Read-only recovery input. The original journal is authoritative for
/// intent/terminal; a terminal ToolRecord alone is not physical quiescence.
pub struct SandboxToolRecovery {
    pub effect_id: String,
    pub agent_ref: String,
    pub sandbox_ref: String,
    pub actor: PrincipalId,
    pub driver: DriverKey,
    pub started_seq: u64,
    pub terminal_ref: SandboxToolTerminalRef,
    pub original: ToolRecord,
    pub terminal: ToolRecord,
    pub source: ToolSourceEvidence,
    pub sources: Vec<EffectiveSourceBinding>,
}

fn authorize_recovery_in(
    tx: &Transaction<'_>,
    manager: &PrincipalId,
    agent_ref: &str,
    turn_id: u64,
    call_id: &str,
) -> Result<(Observation, Claim)> {
    let (session, branch) = agent_owner::key_for_ref(tx, agent_ref)?;
    let key = AgentKey::new(&session, &branch);
    agent_owner::authorize_permission(tx, manager, key, AgentPermission::View)?;
    let observed = observe(tx, agent_ref, turn_id, call_id)?;
    let claim = require_claim(tx, &observed.intent)?;
    let project = super::super::parse_sandbox_ref(&claim.sandbox_ref)?.project;
    super::super::authorize_project(tx, manager, &project, true)?;
    require_current_sandbox_in(tx, key, &claim)?;
    Ok((observed, claim))
}

fn require_current_sandbox_in(tx: &Connection, key: AgentKey<'_>, claim: &Claim) -> Result<()> {
    let current: Option<String> = db(tx.query_row(
        "SELECT sandbox_id FROM agent_state_current WHERE session_id=?1 AND branch_id=?2",
        params![key.session, key.branch],
        |row| row.get(0),
    ))?;
    if current.as_deref() != Some(&claim.sandbox_ref) {
        return Err(Error::Conflict("source effect Sandbox identity changed"));
    }
    Ok(())
}

/// Exclusive owner restart may expose held source-bearing metadata for
/// original journal recovery. Every unresolved Run in the Session must
/// have its own exact source-effect provenance. This is not new admission
/// or physical proof, and does not require an installed SourceDriver.
pub(crate) fn require_owner_recovery_in(
    c: &Connection,
    session: &str,
    branch: &str,
    active_run: Option<&str>,
) -> Result<()> {
    if let Some(run) = active_run {
        let exact: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1 AND session_id=?2 AND branch_id=?3)",
            params![run, session, branch],
            |row| row.get(0),
        ))?;
        if !exact {
            return Err(Error::Conflict(
                "source-bearing Agent active Run identity changed",
            ));
        }
    }
    let mut runs = db(c.prepare_cached(
        "SELECT r.id,r.branch_id,r.turn_id FROM runs r
         JOIN branches b ON b.session_id=r.session_id AND b.id=r.branch_id
         WHERE r.session_id=?1 AND (r.status='running' OR b.active_run IS r.id
           OR EXISTS(SELECT 1 FROM hazards h WHERE h.run_id=r.id AND h.state<>'settled'))",
    ))?;
    let mut rows = db(runs.query([session]))?;
    while let Some(row) = db(rows.next())? {
        let run: String = db(row.get(0))?;
        let branch: String = db(row.get(1))?;
        let turn: u64 = db(row.get(2))?;
        let key = AgentKey::new(session, &branch);
        let bound: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_state_current a
             WHERE a.session_id=?1 AND a.branch_id=?2 AND a.admission IN ('held','open')
               AND EXISTS(SELECT 1 FROM sandbox_source_bindings_current s WHERE s.sandbox_id=a.sandbox_id))",
            params![session, branch], |row| row.get(0),
        ))?;
        if !bound {
            return Err(Error::Denied(
                "unresolved Run has no exact source-bearing Agent",
            ));
        }
        let reference = agent_owner::canonical_ref(c, key)?;
        let mut starts = db(c.prepare_cached(
            "SELECT e.seq,json_extract(e.payload,'$.record.call.id') FROM run_events e
             WHERE e.run_id=?1 AND e.kind='tool_started' AND EXISTS(
               SELECT 1 FROM commit_log l WHERE l.entity_type='source_effect'
                 AND l.kind='source.effect.claimed' AND l.entity_id='source-tool:'||e.seq)
             ORDER BY e.seq LIMIT 513",
        ))?;
        let mut starts = db(starts.query([&run]))?;
        let mut count = 0;
        while let Some(start) = db(starts.next())? {
            count += 1;
            if count > 512 {
                return Err(Error::Limit("original source effect journal exceeds bound"));
            }
            let seq: u64 = db(start.get(0))?;
            let call: String = db(start.get(1))?;
            let observed = observe(c, &reference, turn, &call)?;
            if observed.intent.run_id != run || observed.intent.started_seq != seq {
                return Err(Error::Conflict("source effect startup intent changed"));
            }
            let claim = require_claim(c, &observed.intent)?;
            require_current_sandbox_in(c, key, &claim)?;
        }
        if count == 0 {
            return Err(Error::Denied(
                "unresolved Run has no original source custody",
            ));
        }
    }
    Ok(())
}

fn recovery_sources_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    claim: &Claim,
) -> Result<Vec<EffectiveSourceBinding>> {
    // Recovery does not create a new effect; a subsequently lowered
    // host bound must not prevent inspecting the original admitted set.
    SourceSetLimit::default().admit(claim.sources.len())?;
    if claim.sources.is_empty()
        || claim
            .sources
            .windows(2)
            .any(|p| p[0].name.as_str() >= p[1].name.as_str())
    {
        return Err(Error::Storage("invalid original source effect selection"));
    }
    let names = claim
        .sources
        .iter()
        .map(|source| source.name.clone())
        .collect::<Vec<_>>();
    let bindings = super::super::source_bindings::select_in(
        tx,
        &claim.sandbox_ref,
        Some(&names),
        SourceSetLimit::default(),
        &store.inner.source_drivers.reader()?,
    )?;
    let sources = bindings
        .into_iter()
        .zip(&claim.sources)
        .map(|(binding, mode)| {
            if binding.binding.name != mode.name || !binding.binding.access.permits(mode.access) {
                return Err(Error::Conflict("original source effect binding changed"));
            }
            Ok(EffectiveSourceBinding {
                binding,
                access: mode.access,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if bindings_digest(&sources)? != claim.bindings_digest {
        return Err(Error::Conflict(
            "original source effect binding set changed",
        ));
    }
    claim.driver.validate()?;
    Ok(sources)
}

fn released_in(
    tx: &Transaction<'_>,
    observed: &Observation,
    terminal: &SandboxToolTerminalRef,
) -> Result<Option<SandboxEffectOutcome>> {
    let Some(release) = fact_in::<Release>(tx, &observed.intent.effect_id(), RELEASED)? else {
        return Ok(None);
    };
    if release != Release::new(terminal.clone(), release.outcome)
        || release.outcome == SandboxEffectOutcome::Unknown
    {
        return Err(Error::Storage(
            "source custody release differs from original terminal",
        ));
    }
    Ok(Some(release.outcome))
}

fn reclaim_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    intent: Intent,
    claim: &Claim,
    sources: &[EffectiveSourceBinding],
) -> Result<super::SandboxEffectLease> {
    let effect_id = intent.effect_id();
    let set = crate::SourceMutationSet::from_binding_refs(
        sources.iter().map(|source| &source.binding.binding),
    )?;
    let barrier = store.recover_source_mutation_barrier_in(tx, &set, &effect_id)?;
    Ok(super::SandboxEffectLease {
        barrier,
        actor: actor.clone(),
        sandbox: claim.sandbox_ref.clone(),
        effect_id,
        intent: Some(intent),
        write_capable: sources
            .iter()
            .any(|source| source.access == SourceAccess::ReadWrite),
    })
}

impl SessionStore {
    /// Original ToolResult callback, after the original journal commit.
    /// Trusted host only: no source disclosure or new grant. If a tool did
    /// not use source custody or never recorded positive physical settlement,
    /// do not infer it from its output. Current revocation does not erase
    /// an already attempted effect. Recovery reuses the exact saved proof.
    pub fn sandbox_settle_recorded_tool_source_effect(
        &self,
        actor: &PrincipalId,
        agent_ref: &str,
        turn_id: u64,
        call_id: &str,
    ) -> Result<Option<SandboxEffectOutcome>> {
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source effect settlement requires the exclusive Store owner",
            ));
        }
        let mut connection = self.connection()?;
        let tx =
            db(connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let observed = observe(&tx, agent_ref, turn_id, call_id)?;
        if &observed.intent.actor != actor {
            return Err(Error::Denied("original source effect actor differs"));
        }
        let Some(_) = claim_in(&tx, &observed.intent.effect_id())? else {
            return Ok(None);
        };
        let claim = require_claim(&tx, &observed.intent)?;
        let (terminal_ref, _) = terminal_in(&tx, &observed)?;
        if let Some(outcome) = released_in(&tx, &observed, &terminal_ref)? {
            return Ok(Some(outcome));
        }
        let Some(outcome) =
            fact_in::<SandboxEffectOutcome>(&tx, &observed.intent.effect_id(), QUIESCENT)?
        else {
            return Ok(None);
        };
        if outcome == SandboxEffectOutcome::Unknown {
            return Err(Error::Storage("invalid source quiescence fact"));
        }
        let (session, branch) = agent_owner::key_for_ref(&tx, agent_ref)?;
        require_current_sandbox_in(&tx, AgentKey::new(&session, &branch), &claim)?;
        let sources = recovery_sources_in(self, &tx, &claim)?;
        let lease = reclaim_in(self, &tx, actor, observed.intent, &claim, &sources)?;
        lease.settle_in(&tx, outcome)?;
        db(tx.commit())?;
        Ok(Some(outcome))
    }

    /// Recover only original journal-linked source custody. The trusted
    /// inspector independently authorizes source disclosure and proves
    /// factual quiescence/no-start for the original effect. Never launch,
    /// replan or mint a terminal receipt here. If ToolFinished is absent,
    /// only the original settled Run's exact Interrupted Turn may close
    /// the logical side; physical proof is still required. The proof must
    /// establish permanent quiescence of the original effect, not a
    /// transient empty process scan. Broader Run hazards are not changed.
    pub fn sandbox_recover_tool_source_effect(
        &self,
        manager: &PrincipalId,
        agent_ref: &str,
        turn_id: u64,
        call_id: &str,
        inspect: impl FnOnce(&SandboxToolRecovery) -> Result<SandboxEffectOutcome>,
    ) -> Result<SandboxEffectOutcome> {
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source effect recovery requires the exclusive Store owner",
            ));
        }
        let (lease, recovery) = {
            let mut connection = self.connection()?;
            let tx =
                db(connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
            let (observed, claim) =
                authorize_recovery_in(&tx, manager, agent_ref, turn_id, call_id)?;
            let (terminal_ref, terminal) = terminal_in(&tx, &observed)?;
            if let Some(outcome) = released_in(&tx, &observed, &terminal_ref)? {
                return Ok(outcome);
            }
            let recorded =
                fact_in::<SandboxEffectOutcome>(&tx, &observed.intent.effect_id(), QUIESCENT)?;
            let intent = observed.intent;
            let sources = recovery_sources_in(self, &tx, &claim)?;
            let lease = reclaim_in(self, &tx, manager, intent.clone(), &claim, &sources)?;
            if let Some(outcome) = recorded {
                if outcome == SandboxEffectOutcome::Unknown {
                    return Err(Error::Storage("invalid source quiescence fact"));
                }
                lease.settle_in(&tx, outcome)?;
                db(tx.commit())?;
                return Ok(outcome);
            }
            let recovery = SandboxToolRecovery {
                effect_id: intent.effect_id(),
                agent_ref: intent.agent_ref,
                sandbox_ref: claim.sandbox_ref,
                actor: intent.actor,
                driver: claim.driver,
                started_seq: intent.started_seq,
                terminal_ref,
                original: observed.record,
                terminal,
                source: observed.source,
                sources,
            };
            db(tx.commit())?;
            (lease, recovery)
        };
        match inspect(&recovery) {
            Ok(outcome) => {
                lease.settle(outcome)?;
                Ok(outcome)
            }
            Err(error) => {
                lease.settle(SandboxEffectOutcome::Unknown)?;
                Err(error)
            }
        }
    }
}
