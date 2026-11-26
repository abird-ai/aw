//! Exact logical reconciliation of one original direct source Run.
//! Physical recovery stays with each existing source-effect owner. No
//! provider receipt, immutable Turn, source release or grant is rewritten.
use super::*;
use crate::{
    actor_admission::InputAdmissionState,
    sandbox_owner::{self, SandboxLifecycle, SandboxSnapshot},
    secret_store::SecretStore,
    QueueState, StoreMode,
};
use aw_agent_core::tool::ToolClaimRoute;
use aw_tools::{
    filesystem::{access::Action, schema::Tool},
    OperationId,
};
use std::collections::BTreeSet;

const NATIVE_NOTE: &str =
    "Original native source releases or claim-gated no-start verified; original Run retained, no replay";
const PROCESS_NOTE: &str =
    "Original Process source releases or claim-gated no-start verified; original Run retained, no replay";
const MIXED_NOTE: &str =
    "Original native and Process source releases or claim-gated no-start verified; original Run retained, no replay";

/// Data-only result of reconciliation, not a new execution receipt.
/// Inspected Input/queue custody is not a successful Turn or provider proof.
#[derive(Debug, Serialize)]
pub struct SourceTurnReconciliation {
    pub turn: u64,
    pub source_effects: usize,
    pub unclaimed_no_start: usize,
    pub hazard_cleared: bool,
    pub input: InputAdmissionState,
    pub queue: Option<QueueState>,
    pub changed: bool,
}

/// Compatibility name for the original native-only entry point.
pub type NativeTurnReconciliation = SourceTurnReconciliation;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryScope {
    NativeOnly,
    DirectSources,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum IntentRoute {
    Native,
    Process(OperationId),
    Graph {
        marker: ToolClaimRoute,
        process: Option<OperationId>,
    },
}

impl IntentRoute {
    fn from_observed(observed: &Observation) -> Result<Self> {
        let record = &observed.record;
        if record.call.name == "batch" && record.effect == ToolEffect::Control {
            if let Some(marker) = observed
                .source
                .claim_route
                .filter(|marker| marker.is_source_graph())
            {
                let graph = super::node::original_graph(observed)?;
                return Ok(Self::Graph {
                    marker,
                    process: super::node::graph_process(&graph)?,
                });
            }
        }
        if let Some(tool) = Tool::from_name(&record.call.name) {
            let effect = if tool.action() == Action::Read {
                ToolEffect::Read
            } else {
                ToolEffect::Write
            };
            if tool != Tool::Remove
                && record.effect == effect
                && matches!(
                    observed.source.claim_route,
                    None | Some(ToolClaimRoute::NativeSourceV1)
                )
            {
                return Ok(Self::Native);
            }
        } else if let Some(
            operation @ (OperationId::Bash | OperationId::Pwsh | OperationId::Powershell),
        ) = OperationId::from_model_name(&record.call.name)
        {
            // Only the original fixed foreground Process constructor,
            // never a legacy shell, generic Job/Graph, or extension.
            if record.effect == ToolEffect::Process
                && observed.source.claim_route == Some(ToolClaimRoute::ProcessSourceV1)
            {
                return Ok(Self::Process(operation));
            }
        }
        Err(Error::Denied("Run contains an unsupported source intent"))
    }

    fn marker(self) -> ToolClaimRoute {
        match self {
            Self::Native => ToolClaimRoute::NativeSourceV1,
            Self::Process(_) => ToolClaimRoute::ProcessSourceV1,
            Self::Graph { marker, .. } => marker,
        }
    }
}

pub(super) struct IntentSummary {
    pub count: usize,
    pub possible_effect: bool,
    native: bool,
    legacy_native: bool,
    graph: Option<ToolClaimRoute>,
    process: Option<OperationId>,
}

impl IntentSummary {
    fn observe_route(
        &mut self,
        route: IntentRoute,
        observed: &Observation,
        scope: RecoveryScope,
    ) -> Result<()> {
        match route {
            IntentRoute::Native => {
                self.native = true;
                self.legacy_native |=
                    observed.source.claim_route != Some(ToolClaimRoute::NativeSourceV1);
            }
            IntentRoute::Graph { marker, process } => {
                if scope == RecoveryScope::NativeOnly
                    || self.graph.is_some_and(|old| old != marker)
                    || process.is_some_and(|new| self.process.is_some_and(|old| old != new))
                {
                    return Err(Error::Denied(
                        "Run differs from its original fixed Graph route",
                    ));
                }
                // The qualified Graph host still uses its original Git layout.
                self.native = true;
                self.graph = Some(marker);
                if process.is_some() {
                    self.process = process;
                }
            }
            IntentRoute::Process(operation) => {
                if scope == RecoveryScope::NativeOnly
                    || self.process.is_some_and(|old| old != operation)
                {
                    return Err(Error::Denied(
                        "Run differs from its original fixed source route",
                    ));
                }
                self.process = Some(operation);
            }
        }
        if self.graph == Some(ToolClaimRoute::NativeGraphSourceV1) && self.process.is_some() {
            return Err(Error::Denied(
                "original source route cannot mix native Graph and Process",
            ));
        }
        if (self.graph.is_some() || self.process.is_some()) && self.legacy_native {
            return Err(Error::Denied(
                "mixed source Run contains an unmarked native intent",
            ));
        }
        Ok(())
    }

    pub(super) fn require_layout(&self, sandbox: &SandboxSnapshot) -> Result<()> {
        if self.native || self.process.is_none() {
            // Preserve original native and pre-tool Git restriction,
            // including mixed direct native/Process runs.
            require_git_layout(sandbox)
        } else if sandbox.sources.is_empty() {
            Err(Error::Denied(
                "Process Run recovery requires its original source layout",
            ))
        } else {
            Ok(())
        }
    }

    fn note(&self) -> &'static str {
        if self.graph.is_some() {
            return "Original Graph node source releases or claim-gated no-start verified; original Run retained, no replay";
        }
        match (self.native, self.process.is_some()) {
            (true, true) => MIXED_NOTE,
            (false, true) => PROCESS_NOTE,
            _ => NATIVE_NOTE,
        }
    }
}

/// Only the original durable, fixed executor contract proves that this
/// intent could not enter its worker or OS launch without its claim.
/// Generic trusted-host identity or absent custody is insufficient.
pub(super) fn require_unclaimed_no_start(
    c: &Connection,
    observed: &Observation,
    route: IntentRoute,
) -> Result<()> {
    if matches!(route, IntentRoute::Graph { .. }) {
        return super::node::require_unclaimed_graph(c, observed);
    }
    let effect_id = observed.intent.effect_id();
    if observed.source.claim_route != Some(route.marker())
        || claim_in(c, &effect_id)?.is_some()
        || fact_in::<SandboxEffectOutcome>(c, &effect_id, QUIESCENT)?.is_some()
        || fact_in::<Release>(c, &effect_id, RELEASED)?.is_some()
        || observed
            .terminal
            .as_ref()
            .is_some_and(|(_, record)| record.status == ToolStatus::Succeeded)
    {
        return Err(Error::Conflict(
            "unclaimed intent lacks positive original no-start evidence",
        ));
    }
    let leased: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_leases
          WHERE scope_kind='execution' AND scope_key GLOB 'source-mutation:*' AND run_id=?1)",
        [&effect_id],
        |row| row.get(0),
    ))?;
    if leased {
        return Err(Error::Conflict(
            "unclaimed source intent has contradictory source custody",
        ));
    }
    Ok(())
}

pub(super) fn require_git_layout(sandbox: &SandboxSnapshot) -> Result<()> {
    let git = DriverKey::builtin("git")?;
    if sandbox.sources.is_empty() || sandbox.sources.iter().any(|source| source.driver != git) {
        return Err(Error::Denied(
            "native Run recovery requires its original Git source layout",
        ));
    }
    Ok(())
}

/// One bounded original intent scan for startup and terminal reconciliation.
pub(super) fn inspect_source_intents(
    c: &Connection,
    run: &str,
    agent: &str,
    turn: u64,
    actor: &PrincipalId,
    scope: RecoveryScope,
    mut inspect: impl FnMut(&Observation, IntentRoute) -> Result<()>,
) -> Result<IntentSummary> {
    let mut query = db(c.prepare_cached(
        "SELECT seq,payload FROM run_events WHERE run_id=?1 ORDER BY seq LIMIT 513",
    ))?;
    let mut rows = db(query.query([run]))?;
    let mut starts = BTreeSet::new();
    let mut summary = IntentSummary {
        count: 0,
        possible_effect: false,
        native: false,
        legacy_native: false,
        graph: None,
        process: None,
    };
    let mut count = 0;
    while let Some(row) = db(rows.next())? {
        count += 1;
        if count > 512 {
            return Err(Error::Limit("original Run journal exceeds bound"));
        }
        let seq: u64 = db(row.get(0))?;
        match crate::decode::<JournalEntry>(&db(row.get::<_, String>(1))?)? {
            JournalEntry::ToolStarted { record, .. } => {
                if record.execution_site != "local" || !starts.insert(record.call.id.clone()) {
                    return Err(Error::Denied(
                        "Run contains an unsupported or ambiguous source intent",
                    ));
                }
                let observed = observe(c, agent, turn, &record.call.id)?;
                if observed.intent.run_id != run
                    || observed.intent.started_seq != seq
                    || &observed.intent.actor != actor
                    || !observed.source.is_trusted_host()
                {
                    return Err(Error::Denied("source intent differs from original Run"));
                }
                let route = IntentRoute::from_observed(&observed)?;
                // Fixed marked native routes may coexist with one
                // foreground Process constructor. Never mix shell names
                // or reinterpret an unmarked legacy tool.
                summary.observe_route(route, &observed, scope)?;
                inspect(&observed, route)?;
                summary.possible_effect |= record.effect != ToolEffect::Read;
            }
            JournalEntry::ToolFinished { record } if !starts.contains(&record.call.id) => {
                return Err(Error::Conflict(
                    "original ToolFinished has no matching intent",
                ))
            }
            JournalEntry::ToolRevised { .. } => {
                return Err(Error::Denied(
                    "source Run recovery does not admit revised tool intents",
                ))
            }
            _ => {}
        }
    }
    summary.count = starts.len();
    Ok(summary)
}

/// Require each original effect's positive release or exact gated no-start.
/// Neither case manufactures physical settlement, or changes old history.
fn require_releases(
    c: &Transaction<'_>,
    run: &str,
    agent: &str,
    sandbox: &SandboxSnapshot,
    turn: u64,
    actor: &PrincipalId,
    scope: RecoveryScope,
) -> Result<(IntentSummary, usize, usize)> {
    let native = DriverKey::builtin("native-filesystem")?;
    // The original complete versioned binding digest is sufficient for
    // logical recovery: do not require a current driver or physical probe.
    let bindings =
        sandbox_owner::source_bindings::versioned_bindings_in(c, &sandbox.canonical_ref)?;
    let mut no_start = 0;
    let mut effects = 0;
    let summary = inspect_source_intents(c, run, agent, turn, actor, scope, |observed, route| {
        if matches!(route, IntentRoute::Graph { .. }) {
            let (claimed, unclaimed) = super::node::require_graph_releases(c, observed, sandbox)?;
            effects += claimed;
            no_start += unclaimed;
            return Ok(());
        }
        if observed.active {
            return Err(Error::Denied("source Run is still active"));
        }
        if claim_in(c, &observed.intent.effect_id())?.is_none() {
            require_unclaimed_no_start(c, observed, route)?;
            let (_, record) =
                terminal_in_with(c, observed, observed.record.effect != ToolEffect::Read)?;
            if record.status == ToolStatus::Succeeded {
                return Err(Error::Conflict(
                    "unclaimed source intent cannot have succeeded",
                ));
            }
            no_start += 1;
            return Ok(());
        }
        let claim = require_claim(c, &observed.intent)?;
        if (route == IntentRoute::Native && claim.driver != native)
            || claim.sandbox_ref != sandbox.canonical_ref
            || claim.sources.len() != sandbox.sources.len()
        {
            return Err(Error::Denied(
                "Run effect differs from original complete source custody",
            ));
        }
        verify_claim_bindings(&claim, bindings.clone())?;
        let (terminal, record) = terminal_in(c, observed)?;
        match released_in(c, observed, &terminal)? {
            Some(SandboxEffectOutcome::NotStarted) if record.status == ToolStatus::Succeeded => {
                return Err(Error::Conflict(
                    "no-start release contradicts original success",
                ));
            }
            Some(_) => {
                effects += 1;
            }
            None => {
                return Err(Error::Conflict(
                    "original effect has no positive source release",
                ))
            }
        }
        Ok(())
    })?;
    summary.require_layout(sandbox)?;
    Ok((summary, effects, no_start))
}

impl SessionStore {
    /// Reconcile only a terminal, attached direct native Run from a
    /// previous exclusive owner. All original effects must already have
    /// positive release facts, exact original claim-gated no-start, or no
    /// tool intent. Current owner/writer
    /// and Agent/Project authority are rechecked in the same transaction.
    /// Preserve provider uncertainty, independent hazards and history.
    pub fn reconcile_native_turn(
        &self,
        manager: &PrincipalId,
        agent_ref: &str,
        turn: u64,
    ) -> Result<NativeTurnReconciliation> {
        self.reconcile_source_turn_in(manager, agent_ref, turn, RecoveryScope::NativeOnly)
    }

    /// Original direct native and/or fixed foreground source Process Run.
    /// Require all original positive releases (including claimed no-spawn)
    /// or exact claim-gated unclaimed no-start; no physical inspection,
    /// new Process launch, or reinterpretation of the immutable Turn.
    pub fn reconcile_source_turn(
        &self,
        manager: &PrincipalId,
        agent_ref: &str,
        turn: u64,
    ) -> Result<SourceTurnReconciliation> {
        self.reconcile_source_turn_in(manager, agent_ref, turn, RecoveryScope::DirectSources)
    }

    fn reconcile_source_turn_in(
        &self,
        manager: &PrincipalId,
        agent_ref: &str,
        turn: u64,
        scope: RecoveryScope,
    ) -> Result<SourceTurnReconciliation> {
        label(agent_ref, 2048)?;
        if turn == 0 || self.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source Turn recovery requires exact Turn and exclusive Store owner",
            ));
        }
        let process = self
            .process_identity()?
            .ok_or(Error::Denied("missing current Store owner"))?;
        let (result, payload) = {
            let mut c = self.connection()?;
            let tx = db(c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
            let (session, branch) = agent_owner::key_for_ref(&tx, agent_ref)?;
            let key = AgentKey::new(&session, &branch);
            if agent_owner::canonical_ref(&tx, key)? != agent_ref {
                return Err(Error::Denied(
                    "source recovery requires exact canonical Agent",
                ));
            }
            agent_owner::authorize_permission(&tx, manager, key, AgentPermission::Manage)?;
            let (project, layout, sandbox, _, lifecycle) =
                sandbox_owner::agent_project_and_layout(&tx, &session, &branch)?;
            sandbox_owner::authorize_project(&tx, manager, &project, true)?;
            let sandbox = sandbox_owner::snapshot_with_sources(
                &tx,
                &sandbox.ok_or(Error::Denied("source recovery requires bound Sandbox"))?,
            )?;
            if lifecycle != "open"
                || sandbox.lifecycle != SandboxLifecycle::Open
                || sandbox.project_ref != project.canonical()
                || sandbox.layout_digest != layout.digest
                || sandbox.tool_root.to_str() != Some(layout.tool_root.as_str())
                || sandbox.working_directory.to_str() != Some(layout.working_directory.as_str())
            {
                return Err(Error::Denied(
                    "source recovery differs from original Agent layout",
                ));
            }

            let row: Option<(String, String, String, bool)> = db(tx.query_row(
                "SELECT r.id,r.final_node,r.root_key,r.possible_effect FROM runs r
                 JOIN local_process_incarnations p ON p.process_id=r.owner_process_id
                 WHERE r.session_id=?1 AND r.branch_id=?2 AND r.turn_id=?3
                   AND r.status IN ('completed','failed','cancelled','incomplete','interrupted')
                   AND r.ended_at IS NOT NULL AND r.final_node IS NOT NULL AND r.attach_status='attached'
                   AND p.state IN ('stopped','fenced') AND p.process_id<>?4
                   AND NOT EXISTS(SELECT 1 FROM branches b WHERE b.session_id=?1 AND b.active_run IS NOT NULL)
                   AND NOT EXISTS(SELECT 1 FROM runs live WHERE live.session_id=?1 AND live.status='running')",
                params![session, branch, turn, process.process_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).optional())?;
            let (run, final_node, root_key, possible) = row.ok_or(Error::Denied(
                "source recovery requires original attached Run of a previous quiescent Store owner",
            ))?;
            let original = direct_run_in(&tx, &session, &branch, &run, turn)?;
            let node = crate::node_in(&tx, &session, &final_node)?;
            if node.kind != "turn"
                || node.parent_id.as_deref() != Some(original.prompt_node.as_str())
                || node.actor != original.actor
                || node.payload["id"].as_u64() != Some(turn)
                || crate::key(&crate::session_in(&tx, &session)?.binding)? != root_key
            {
                return Err(Error::Conflict("settled source Run identity changed"));
            }
            let (summary, source_effects, unclaimed_no_start) =
                require_releases(&tx, &run, agent_ref, &sandbox, turn, &original.actor, scope)?;
            let effectful = summary.possible_effect;
            let note = summary.note();
            if effectful != possible {
                return Err(Error::Conflict(
                    "original Run mutation intent differs from its hazard",
                ));
            }
            // Never clear another Run's hazard, import review or provider
            // attempt. The immutable source releases prove this one only.
            let hazard: Option<(String, String, String)> = db(tx
                .query_row(
                    "SELECT session_id,root_key,state FROM hazards WHERE run_id=?1",
                    [&run],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional())?;
            let hazard_cleared = match hazard {
                None if !effectful => false,
                Some((s, root, state)) if effectful && s == session && root == root_key => {
                    match state.as_str() {
                        "settled" => false,
                        "needs_inspection" => {
                            db(tx.execute(
                                "UPDATE hazards SET state='settled',note=?1,updated_at=?2
                             WHERE run_id=?3 AND state='needs_inspection'",
                                params![note, crate::now(), run],
                            ))?;
                            crate::audit(
                                &tx,
                                &session,
                                manager,
                                "effects.inspected",
                                serde_json::json!({"run_id":run,"note":note}),
                            )?;
                            true
                        }
                        _ => return Err(Error::Conflict("original Run hazard not terminal")),
                    }
                }
                _ => return Err(Error::Conflict("original Run hazard identity differs")),
            };
            let (input, input_changed) =
                crate::actor_admission::inspect_recovered_in(&tx, manager, &original.input, note)?;
            let (queue, queue_changed) = crate::actor_queue::retire_recovered_in(
                &tx,
                manager,
                &original.input,
                &original.process_id,
                process.store_generation,
                input,
                note,
            )?;
            let changed = hazard_cleared || input_changed || queue_changed;
            if changed {
                agent_owner::changed(&tx, key)?;
            }
            let result = SourceTurnReconciliation {
                turn,
                source_effects,
                unclaimed_no_start,
                hazard_cleared,
                input,
                queue: queue.as_ref().map(|fact| fact.state),
                changed,
            };
            db(tx.commit())?;
            (result, queue.map(|fact| fact.payload_ref))
        };
        // The original immutable terminal queue fact precedes private
        // payload retirement. Exact replay can finish a missed unlink.
        if let Some(payload) = payload {
            self.local_secret_store()?.delete(&payload)?;
        }
        Ok(result)
    }
}
