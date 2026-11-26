//! Original foreground native Graph node custody. Outer ToolStarted owns
//! constructor intent; each actual sealed node owns its individual source
//! claim and original JobManager terminal commit. No second Job journal.
use super::*;
use aw_agent_core::{tool::ToolClaimRoute, ToolSource};
use aw_tools::{
    filesystem::{
        access::Action,
        batch::{Graph, NodeTool},
        job::{Job, Transition},
        job_manager::{NodeExecutionBinding, RunningNode},
        schema::Tool,
        status::Primary,
    },
    task_fence::TaskIdentity,
    OperationId,
};

pub(super) fn original_graph(observed: &Observation) -> Result<Graph> {
    let marker = observed
        .source
        .claim_route
        .filter(|marker| marker.is_source_graph())
        .ok_or(Error::Denied(
            "original Graph source constructor unavailable",
        ))?;
    if observed.record.call.name != "batch"
        || observed.record.effect != ToolEffect::Control
        || observed.record.execution_site != "local"
        || !observed.source.is_trusted_host()
    {
        return Err(Error::Denied(
            "original Graph source constructor unavailable",
        ));
    }
    let graph = Graph::compile_with(
        &observed.record.call.arguments,
        |name| OperationId::from_model_name(name).and_then(NodeTool::from_operation),
        |tool, _| {
            tool.native()
                .is_some_and(|tool| tool != Tool::Remove && tool.graph_eligible())
                || (marker == ToolClaimRoute::ProcessGraphSourceV1 && tool.is_shell())
        },
    )
    .map_err(|_| Error::Denied("original fixed source Graph is invalid"))?;
    graph_process(&graph)?;
    Ok(graph)
}

pub(super) fn graph_process(graph: &Graph) -> Result<Option<OperationId>> {
    let mut process = None;
    for step in graph.steps() {
        if step.tool.is_shell() {
            let operation = step.tool.operation();
            if process.is_some_and(|old| old != operation) {
                return Err(Error::Denied("original Graph has multiple shell routes"));
            }
            process = Some(operation);
        }
    }
    Ok(process)
}

fn node_effect(tool: NodeTool) -> (ToolEffect, ToolClaimRoute) {
    if tool.is_shell() {
        (ToolEffect::Process, ToolClaimRoute::ProcessSourceV1)
    } else if tool
        .native()
        .is_some_and(|tool| tool.action() == Action::Read)
    {
        (ToolEffect::Read, ToolClaimRoute::NativeSourceV1)
    } else {
        (ToolEffect::Write, ToolClaimRoute::NativeSourceV1)
    }
}

fn original_task(c: &Connection, observed: &Observation) -> Result<TaskIdentity> {
    let (session, branch) = agent_owner::key_for_ref(c, &observed.intent.agent_ref)?;
    Ok(TaskIdentity {
        actor: observed.intent.actor.as_str().into(),
        source_kind: observed.source.kind.clone(),
        source_id: observed.source.id.clone(),
        source_epoch: observed.source.epoch,
        execution_site: observed.record.execution_site.clone(),
        session,
        branch,
        turn_id: observed.intent.turn_id,
        call_id: observed.intent.call_id.clone(),
        effect: aw_tools::ToolEffect::Control,
    })
}

fn require_binding(
    graph: &Graph,
    graph_hash: [u8; 32],
    task: &TaskIdentity,
    binding: &NodeExecutionBinding,
) -> Result<()> {
    if binding.coordinate == 0
        || binding.coordinate > graph.steps().len()
        || binding.running_sequence < 3
        || binding.plan_identity == [0; 32]
        || binding.graph_hash != graph_hash
        || Job::task_run_binding(task, &binding.job_id, binding.graph_hash)
            != Ok(binding.run_binding)
    {
        return Err(Error::Conflict(
            "Graph node differs from original sealed Job",
        ));
    }
    Ok(())
}

fn intent_for(observed: &Observation, binding: &NodeExecutionBinding) -> Intent {
    let mut intent = observed.intent.clone();
    intent.node = Some(binding.clone());
    intent
}

fn key(observed: &Observation, coordinate: usize) -> String {
    format!("{}:{coordinate}", observed.intent.effect_id())
}

fn same_job(a: &NodeExecutionBinding, b: &NodeExecutionBinding) -> bool {
    a.job_id == b.job_id && a.run_binding == b.run_binding && a.graph_hash == b.graph_hash
}

/// All nodes in one original constructor belong to the same actual Job,
/// even across separate source claim/settlement transactions.
fn require_sibling_job(
    c: &Connection,
    observed: &Observation,
    node: &NodeExecutionBinding,
) -> Result<()> {
    let prefix = format!("{}:*", observed.intent.effect_id());
    let existing: Option<String> = db(c
        .query_row(
            "SELECT entity_id FROM commit_log WHERE entity_type='source_effect' AND kind=?1
         AND entity_id GLOB ?2 LIMIT 1",
            params![CLAIMED, prefix],
            |row| row.get(0),
        )
        .optional())?;
    if let Some(existing) = existing {
        let claim = claim_in(c, &existing)?
            .ok_or(Error::Conflict("original Graph source claim unavailable"))?;
        let other = claim
            .intent
            .node
            .as_ref()
            .ok_or(Error::Conflict("original Graph node identity missing"))?;
        if claim.intent != intent_for(observed, other) || !same_job(other, node) {
            return Err(Error::Conflict(
                "Graph source claims belong to different Jobs",
            ));
        }
    }
    Ok(())
}

pub(in crate::sandbox_owner::effects) fn admit_in(
    tx: &Transaction<'_>,
    request: &SandboxEffectRequest,
    node: &RunningNode,
    binding: &ToolBindingSnapshot,
    constructor: &ToolBindingSnapshot,
    driver: &DriverKey,
    admitted: Option<&Intent>,
) -> Result<Intent> {
    let task = node.task();
    let observed = observe(tx, &request.agent_ref, task.turn_id, &task.call_id)?;
    let graph = original_graph(&observed)?;
    let expected_task = original_task(tx, &observed)?;
    if !observed.active
        || observed.terminal.is_some()
        || observed.intent.actor != request.actor
        || task != &expected_task
        || constructor.name != "batch"
        || constructor.effect != ToolEffect::Control
        || constructor.source != ToolSource::trusted_host()
        || constructor.execution_site != "local"
        || constructor.claim_route != observed.source.claim_route
        || !constructor
            .claim_route
            .zip(binding.claim_route)
            .is_some_and(|(constructor, peer)| constructor.permits_graph_peer(peer))
        || binding.source != ToolSource::trusted_host()
        || binding.execution_site != "local"
    {
        return Err(Error::Denied(
            "Graph node requires its original constructor and peer authority",
        ));
    }
    require_binding(&graph, graph.fingerprint(), task, node.binding())?;
    let plan = node.plan();
    let step = &graph.steps()[node.binding().coordinate - 1];
    let (effect, marker) = node_effect(step.tool);
    if (marker == ToolClaimRoute::NativeSourceV1
        && driver != &DriverKey::builtin("native-filesystem")?)
        || (marker == ToolClaimRoute::ProcessSourceV1
            && driver == &DriverKey::builtin("native-filesystem")?)
    {
        return Err(Error::Denied(
            "Graph node differs from registered source driver route",
        ));
    }
    if plan.coordinate() != node.binding().coordinate
        || plan.replay_identity() != node.binding().plan_identity
        || plan.tool() != step.tool
        || plan.arguments() != &step.arguments
        || binding.name != step.tool.name()
        || binding.effect != effect
        || binding.claim_route != Some(marker)
        || (effect == ToolEffect::Read
            && request
                .sources
                .iter()
                .any(|source| source.access == SourceAccess::ReadWrite))
    {
        return Err(Error::Denied(
            "Graph node differs from original plan or effect",
        ));
    }
    let (session, _) = agent_owner::key_for_ref(tx, &request.agent_ref)?;
    crate::workspace_writer::verify_writer_epoch_for_session(tx, &session, observed.writer_epoch)?;
    if claim_in(tx, &observed.intent.effect_id())?.is_some() {
        return Err(Error::Conflict(
            "Graph constructor cannot own a direct source claim",
        ));
    }
    require_sibling_job(tx, &observed, node.binding())?;
    if effect == ToolEffect::Process {
        for (index, step) in graph.steps().iter().enumerate() {
            if step.tool.is_shell() {
                if let Some(claim) = claim_in(tx, &key(&observed, index + 1))? {
                    if claim.driver != *driver {
                        return Err(Error::Conflict("original Graph Process driver changed"));
                    }
                }
            }
        }
    }
    let intent = intent_for(&observed, node.binding());
    require_launchable_intent(tx, &intent, admitted)?;
    if admitted.is_some() && require_claim(tx, &intent)?.driver != *driver {
        return Err(Error::Conflict("original Graph node driver changed"));
    }
    Ok(intent)
}

fn node_claim(
    c: &Connection,
    observed: &Observation,
    graph: &Graph,
    graph_hash: [u8; 32],
    task: &TaskIdentity,
    coordinate: usize,
) -> Result<Option<Claim>> {
    let Some(claim) = claim_in(c, &key(observed, coordinate))? else {
        return Ok(None);
    };
    let node = claim
        .intent
        .node
        .as_ref()
        .ok_or(Error::Conflict("original Graph node identity missing"))?;
    require_binding(graph, graph_hash, task, node)?;
    if node.coordinate != coordinate
        || claim.intent != intent_for(observed, node)
        || claim.store_generation != StoreScope::load(c)?.restore_generation()
    {
        return Err(Error::Conflict(
            "Graph source claim differs from original node",
        ));
    }
    let (effect, marker) = node_effect(graph.steps()[coordinate - 1].tool);
    let native_driver = DriverKey::builtin("native-filesystem")?;
    if (marker == ToolClaimRoute::NativeSourceV1 && claim.driver != native_driver)
        || (marker == ToolClaimRoute::ProcessSourceV1 && claim.driver == native_driver)
        || (effect == ToolEffect::Read
            && claim
                .sources
                .iter()
                .any(|source| source.access == SourceAccess::ReadWrite))
    {
        return Err(Error::Conflict(
            "Graph node claim differs from original effect/driver route",
        ));
    }
    claim.driver.validate()?;
    Ok(Some(claim))
}

pub(super) fn recovery_claim_in(
    c: &Connection,
    observed: &Observation,
    coordinate: usize,
) -> Result<(Claim, SandboxGraphNodeRecovery)> {
    let graph = original_graph(observed)?;
    let task = original_task(c, observed)?;
    let claim = node_claim(c, observed, &graph, graph.fingerprint(), &task, coordinate)?
        .ok_or(Error::Conflict("original Graph node has no source custody"))?;
    let binding = claim
        .intent
        .node
        .as_ref()
        .ok_or(Error::Conflict("original Graph node identity missing"))?;
    require_sibling_job(c, observed, binding)?;
    let step = &graph.steps()[coordinate - 1];
    let node = SandboxGraphNodeRecovery {
        binding: binding.clone(),
        tool: step.tool,
        arguments: step.arguments.clone(),
    };
    Ok((claim, node))
}

fn unclaimed(c: &Connection, effect_id: &str) -> Result<()> {
    if fact_in::<SandboxEffectOutcome>(c, effect_id, QUIESCENT)?.is_some()
        || fact_in::<Release>(c, effect_id, RELEASED)?.is_some()
    {
        return Err(Error::Conflict(
            "unclaimed Graph node has contradictory proof",
        ));
    }
    let leased: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_coordination_leases
         WHERE scope_kind='execution' AND scope_key GLOB 'source-mutation:*' AND run_id=?1)",
        [effect_id],
        |row| row.get(0),
    ))?;
    if leased {
        return Err(Error::Conflict(
            "unclaimed Graph node has contradictory source custody",
        ));
    }
    Ok(())
}

fn require_outcome_status(outcome: SandboxEffectOutcome, primary: Primary) -> Result<()> {
    match (outcome, primary) {
        (SandboxEffectOutcome::Settled, Primary::Skip)
        | (SandboxEffectOutcome::NotStarted, Primary::Ok | Primary::Unchanged | Primary::Mixed)
        | (SandboxEffectOutcome::Unknown, _) => Err(Error::Conflict(
            "Graph terminal contradicts original physical proof",
        )),
        _ => Ok(()),
    }
}

pub(super) fn released(
    c: &Connection,
    observed: &Observation,
    claim: &Claim,
) -> Result<Option<SandboxEffectOutcome>> {
    let Some(release) = fact_in::<Release>(c, &claim.intent.effect_id(), RELEASED)? else {
        return Ok(None);
    };
    let node = claim
        .intent
        .node
        .as_ref()
        .ok_or(Error::Conflict("original Graph node identity missing"))?;
    if release.outcome == SandboxEffectOutcome::Unknown {
        return Err(Error::Conflict("invalid Graph source release"));
    }
    let terminal = if let Some(transition) = release.node_transition {
        let status = node.settlement(transition).ok_or(Error::Conflict(
            "source release differs from original node transition",
        ))?;
        require_outcome_status(release.outcome, status.primary)?;
        SandboxToolTerminalRef::GraphNode(transition)
    } else {
        // Recovery may close source custody after original outer logical
        // closure, never invent a missing node terminal transition.
        terminal_in(c, observed)?.0
    };
    if release != Release::new(terminal, release.outcome)
        || fact_in::<SandboxEffectOutcome>(c, &claim.intent.effect_id(), QUIESCENT)?
            != Some(release.outcome)
    {
        return Err(Error::Conflict(
            "Graph source release differs from original proof",
        ));
    }
    Ok(Some(release.outcome))
}

// Graph claims are always on the original complete named source set.
pub(super) fn verify_complete_claim(
    c: &Connection,
    claim: &Claim,
    sandbox: &crate::SandboxSnapshot,
) -> Result<()> {
    if claim.sandbox_ref != sandbox.canonical_ref || claim.sources.len() != sandbox.sources.len() {
        return Err(Error::Conflict(
            "Graph node source set differs from original Sandbox",
        ));
    }
    verify_claim_bindings(
        claim,
        crate::sandbox_owner::source_bindings::versioned_bindings_in(c, &sandbox.canonical_ref)?,
    )?;
    Ok(())
}

fn scan(
    c: &Connection,
    observed: &Observation,
    mut visit: impl FnMut(usize, Option<&Claim>) -> Result<()>,
) -> Result<(usize, usize)> {
    let graph = original_graph(observed)?;
    let graph_hash = graph.fingerprint();
    let task = original_task(c, observed)?;
    let base = observed.intent.effect_id();
    if claim_in(c, &base)?.is_some() {
        return Err(Error::Conflict(
            "Graph constructor has direct source custody",
        ));
    }
    unclaimed(c, &base)?;
    let mut first: Option<NodeExecutionBinding> = None;
    let mut process_driver = None;
    let mut claimed = 0;
    // Bound the entire namespace, rejecting out-of-graph claims.
    let prefix = format!("{base}:*");
    let stored: usize = db(c.query_row(
        "SELECT COUNT(*) FROM commit_log WHERE entity_type='source_effect' AND kind=?1 AND entity_id GLOB ?2",
        params![CLAIMED, prefix], |row| row.get(0),
    ))?;
    if stored > graph.steps().len() {
        return Err(Error::Conflict("Graph contains unexpected source claims"));
    }
    for coordinate in 1..=graph.steps().len() {
        let claim = node_claim(c, observed, &graph, graph_hash, &task, coordinate)?;
        if let Some(claim) = &claim {
            let node = claim.intent.node.as_ref().expect("validated node");
            if first.as_ref().is_some_and(|old| !same_job(old, node)) {
                return Err(Error::Conflict("Graph contains multiple original Jobs"));
            }
            if first.is_none() {
                first = Some(node.clone());
            }
            if graph.steps()[coordinate - 1].tool.is_shell() {
                if process_driver
                    .as_ref()
                    .is_some_and(|driver| driver != &claim.driver)
                {
                    return Err(Error::Conflict(
                        "Graph contains multiple original Process drivers",
                    ));
                }
                process_driver = Some(claim.driver.clone());
            }
            claimed += 1;
        } else {
            unclaimed(c, &key(observed, coordinate))?;
        }
        visit(coordinate, claim.as_ref())?;
    }
    if claimed != stored {
        return Err(Error::Conflict(
            "Graph has unexpected source custody coordinates",
        ));
    }
    Ok((claimed, graph.steps().len() - claimed))
}

pub(super) fn require_unclaimed_graph(c: &Connection, observed: &Observation) -> Result<()> {
    scan(c, observed, |_, claim| {
        if claim.is_some() {
            return Err(Error::Conflict("Graph source node has original custody"));
        }
        Ok(())
    })?;
    if observed
        .terminal
        .as_ref()
        .is_some_and(|(_, record)| record.status == ToolStatus::Succeeded)
    {
        return Err(Error::Conflict("unclaimed Graph cannot have succeeded"));
    }
    Ok(())
}

pub(super) fn require_graph_releases(
    c: &Connection,
    observed: &Observation,
    sandbox: &crate::SandboxSnapshot,
) -> Result<(usize, usize)> {
    if observed.active {
        return Err(Error::Denied("Graph Run is still active"));
    }
    let (_, terminal) = terminal_in(c, observed)?;
    let counts = scan(c, observed, |_, claim| {
        if let Some(claim) = claim {
            verify_complete_claim(c, claim, sandbox)?;
            if released(c, observed, claim)?.is_none() {
                return Err(Error::Conflict(
                    "original Graph node has no positive source release",
                ));
            }
        }
        Ok(())
    })?;
    if counts.0 == 0 && terminal.status == ToolStatus::Succeeded {
        return Err(Error::Conflict("unclaimed Graph cannot have succeeded"));
    }
    Ok(counts)
}

pub(super) fn require_startup_claims(
    c: &Connection,
    observed: &Observation,
    key: AgentKey<'_>,
) -> Result<()> {
    let (claimed, _) = scan(c, observed, |_, claim| {
        if let Some(claim) = claim {
            require_current_sandbox_in(c, key, claim)?;
        }
        Ok(())
    })?;
    if claimed == 0 {
        return Err(Error::Conflict("original Graph source custody missing"));
    }
    Ok(())
}

fn require_release_boundary(
    tx: &Transaction<'_>,
    observed: &Observation,
    claim: &Claim,
) -> Result<()> {
    if released(tx, observed, claim)?.is_some() {
        return Err(Error::Conflict("original Graph node already released"));
    }
    let (session, branch) = agent_owner::key_for_ref(tx, &observed.intent.agent_ref)?;
    require_current_sandbox_in(tx, AgentKey::new(&session, &branch), claim)?;
    let sandbox = crate::sandbox_owner::snapshot_with_sources(tx, &claim.sandbox_ref)?;
    verify_complete_claim(tx, claim, &sandbox)
}

fn append_node_release(
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    intent: &Intent,
    terminal: SandboxToolTerminalRef,
    outcome: SandboxEffectOutcome,
) -> Result<()> {
    if outcome == SandboxEffectOutcome::Unknown
        || fact_in::<SandboxEffectOutcome>(tx, &intent.effect_id(), QUIESCENT)? != Some(outcome)
    {
        return Err(Error::Conflict(
            "Graph source release lacks exact physical proof",
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

fn release_node_in(
    store: &SessionStore,
    tx: &Transaction<'_>,
    actor: &PrincipalId,
    observed: &Observation,
    claim: &Claim,
    terminal: SandboxToolTerminalRef,
    outcome: SandboxEffectOutcome,
) -> Result<()> {
    require_release_boundary(tx, observed, claim)?;
    let sources = recovery_sources_in(store, tx, claim)?;
    let lease = reclaim_in(store, tx, actor, claim.intent.clone(), claim, &sources)?;
    append_node_release(tx, actor, &claim.intent, terminal, outcome)?;
    lease.settle_sources_in(tx, outcome)
}

/// Same source release owner for an offline inspector's already-held
/// lease. Reobserve the exact original claim/closed constructor; never
/// invent the missing JobManager terminal transition. A fresh positive
/// proof records only physical settlement of the original effect.
pub(super) fn release_recovered_in(
    tx: &Transaction<'_>,
    intent: &Intent,
    actor: &PrincipalId,
    outcome: SandboxEffectOutcome,
) -> Result<()> {
    let coordinate = intent
        .node
        .as_ref()
        .ok_or(Error::Conflict("original Graph node identity missing"))?
        .coordinate;
    let observed = observe(tx, &intent.agent_ref, intent.turn_id, &intent.call_id)?;
    let (claim, _) = recovery_claim_in(tx, &observed, coordinate)?;
    if observed.active || claim.intent != *intent {
        return Err(Error::Denied(
            "Graph recovery differs from closed original node",
        ));
    }
    let terminal = terminal_in(tx, &observed)?.0;
    require_release_boundary(tx, &observed, &claim)?;
    match fact_in::<SandboxEffectOutcome>(tx, &intent.effect_id(), QUIESCENT)? {
        Some(recorded) if recorded == outcome && outcome != SandboxEffectOutcome::Unknown => {}
        None if outcome == SandboxEffectOutcome::Settled => {
            record_quiescence_in(tx, intent, actor, outcome)?;
        }
        _ => {
            return Err(Error::Conflict(
                "Graph recovery lacks matching physical settlement",
            ))
        }
    }
    append_node_release(tx, actor, intent, terminal, outcome)
}

impl SessionStore {
    /// Original JobManager settlement callback, BEFORE its local terminal
    /// projection. Commit the exact node release and physical revision once.
    /// No authority is granted here; no missing proof is inferred.
    pub fn sandbox_settle_recorded_graph_node(
        &self,
        actor: &PrincipalId,
        agent_ref: &str,
        running: &RunningNode,
        transition: Transition,
    ) -> Result<()> {
        if self.inner.mode != crate::StoreMode::OwnerLock {
            return Err(Error::Denied(
                "Graph source settlement requires exclusive Store",
            ));
        }
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let observed = observe(
            &tx,
            agent_ref,
            running.task().turn_id,
            &running.task().call_id,
        )?;
        let graph = original_graph(&observed)?;
        let task = original_task(&tx, &observed)?;
        let binding = running.binding();
        if !observed.active || &observed.intent.actor != actor || &task != running.task() {
            return Err(Error::Denied(
                "Graph node settlement differs from original active constructor",
            ));
        }
        require_binding(&graph, graph.fingerprint(), &task, binding)?;
        let status = binding.settlement(transition).ok_or(Error::Conflict(
            "Graph terminal transition differs from original Running node",
        ))?;
        let Some(claim) = node_claim(
            &tx,
            &observed,
            &graph,
            graph.fingerprint(),
            &task,
            binding.coordinate,
        )?
        else {
            unclaimed(&tx, &key(&observed, binding.coordinate))?;
            if !matches!(
                status.primary,
                Primary::Fail | Primary::Unknown | Primary::Skip
            ) {
                return Err(Error::Conflict(
                    "unclaimed Graph node cannot have succeeded",
                ));
            }
            return Ok(());
        };
        if claim.intent != intent_for(&observed, binding) {
            return Err(Error::Conflict(
                "Graph source settlement belongs to another node",
            ));
        }
        let Some(outcome) =
            fact_in::<SandboxEffectOutcome>(&tx, &claim.intent.effect_id(), QUIESCENT)?
        else {
            if status.primary != Primary::Unknown {
                return Err(Error::Conflict(
                    "Graph node source effect remains unresolved",
                ));
            }
            return Ok(());
        };
        require_outcome_status(outcome, status.primary)?;
        release_node_in(
            self,
            &tx,
            actor,
            &observed,
            &claim,
            SandboxToolTerminalRef::GraphNode(transition),
            outcome,
        )?;
        db(tx.commit())
    }
}
