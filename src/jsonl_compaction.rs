//! Small backend-neutral owner composition of JSONL active-tail compaction.
//! The shared Agent SummaryExecutor is the ONLY paid tool-free generator, and
//! SessionCursor owns append-only intent/result/CAS. Both full and minimal
//! frontends call this, instead of copying a second summary execution loop.
use aw_agent_core::{Agent, CompactionPlan, CompactionPolicy, Error, Outcome, Result};
use aw_store_api::SessionCursor;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub async fn compact(
    agent: &mut Agent,
    cursor: &Arc<dyn SessionCursor>,
    policy: &CompactionPolicy,
) -> Result<u64> {
    policy.validate()?;
    let view = cursor.view()?;
    if &view.session.owner != agent.actor()
        || view.session.binding.profile_hint != agent.options().profile
    {
        return Err(Error::Denied(
            "current Agent actor/account differs from JSONL Session",
        ));
    }
    if let Some(request) = cursor.compaction_ready()? {
        let projected = cursor.apply_compaction(&request)?;
        let through = projected.compacted_through;
        agent.load_history(projected, Some(cursor.journal()))?;
        return Ok(through);
    }
    if agent.needs_reconciliation() || view.history.needs_reconciliation {
        return Err(Error::Denied(
            "unresolved effects or an uncertain paid summary require inspection",
        ));
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Storage("system time unavailable"))?
        .as_nanos();
    let request_id = format!("r10_{}_{}", view.history.next_turn_id, at);
    let plan = CompactionPlan::prepare(&view.history, cursor.session_id(), &request_id, policy)?;
    let selected = agent.provider_accounts().select(
        agent.actor(),
        &agent.options().profile,
        &agent.options().model,
    )?;
    let journal = cursor.compaction_journal(plan.clone())?;
    let result = agent
        .summary_executor()?
        .execute(
            &selected,
            &plan.input,
            journal.as_ref(),
            std::future::pending::<()>(),
        )
        .await?;
    if result.outcome != Outcome::Completed {
        return Err(Error::Protocol(
            "summary incomplete; no projection change or paid attempt replay",
        ));
    }
    let projected = cursor.apply_compaction(&plan.request_id)?;
    let through = projected.compacted_through;
    agent.load_history(projected, Some(cursor.journal()))?;
    Ok(through)
}
