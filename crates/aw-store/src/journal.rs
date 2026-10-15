use super::*;
use aw_agent_core::{
    InvocationRecord, JournalEntry, JournalFuture, ToolEffect, ToolRecord, ToolStatus, Turn,
    TurnJournal, TurnStart,
};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone)]
pub struct SessionHandle {
    store: SessionStore,
    actor: PrincipalId,
    session: String,
    cursor: Arc<Mutex<BranchInfo>>,
    pending_web_receipt: Arc<Mutex<Option<u64>>>,
    pending_retry_operation: Arc<Mutex<Option<String>>>,
    pending_retry_policy: Arc<Mutex<Option<crate::RetryPolicy>>>,
    #[cfg(feature = "routing")]
    pending_routing_operation: Arc<Mutex<Option<String>>>,
}

fn authorize_append_or_shared_request(
    c: &Connection,
    actor: &PrincipalId,
    request: &str,
    session: &str,
    branch: &str,
) -> Result<Option<String>> {
    #[cfg(feature = "multiplayer")]
    if let Some(submission) =
        crate::multiplayer::authorize_shared_request_in(c, actor, request, session, branch)?
    {
        return Ok(Some(submission));
    }
    #[cfg(not(feature = "multiplayer"))]
    let _ = (request, branch);
    authorize(c, session, actor, true).map(|()| None)
}

fn authorize_append_or_shared_receipt(
    c: &Connection,
    actor: &PrincipalId,
    receipt: Option<u64>,
    session: &str,
    branch: &str,
    prompt: Option<&str>,
) -> Result<Option<String>> {
    #[cfg(feature = "multiplayer")]
    if let Some(receipt) = receipt {
        if let Some(submission) = crate::multiplayer::authorize_shared_receipt_in(
            c, actor, receipt, session, branch, prompt,
        )? {
            return Ok(Some(submission));
        }
    }
    #[cfg(not(feature = "multiplayer"))]
    let _ = (receipt, branch, prompt);
    authorize(c, session, actor, true).map(|()| None)
}

fn authorize_append_or_current_shared_submit(
    c: &Connection,
    actor: &PrincipalId,
    session: &str,
) -> Result<()> {
    match authorize(c, session, actor, true) {
        Ok(()) => Ok(()),
        Err(Error::Denied(_)) => {
            #[cfg(feature = "multiplayer")]
            if crate::multiplayer::draft_submit_authorized_in(c, session, actor)? {
                return Ok(());
            }
            Err(Error::Denied(
                "session append or current shared submit authority required",
            ))
        }
        Err(error) => Err(error),
    }
}

fn authorize_append_or_shared_run(
    c: &Connection,
    actor: &PrincipalId,
    run: &str,
    session: &str,
) -> Result<()> {
    #[cfg(feature = "multiplayer")]
    if crate::multiplayer::authorize_shared_run_in(c, actor, run, session)? {
        return Ok(());
    }
    #[cfg(not(feature = "multiplayer"))]
    let _ = run;
    authorize(c, session, actor, true)
}
fn same_retry_destination(
    previous: &aw_agent_core::InvocationIdentity,
    current: &aw_agent_core::InvocationIdentity,
) -> bool {
    previous.actor == current.actor
        && previous.account_id == current.account_id
        && previous.account_owner == current.account_owner
        && previous.payer == current.payer
        && previous.credential_custody == current.credential_custody
        && previous.inference_executor == current.inference_executor
        && previous.model == current.model
}

impl SessionHandle {
    pub(crate) fn new(
        store: SessionStore,
        actor: PrincipalId,
        session: String,
        branch: BranchInfo,
    ) -> Self {
        Self {
            store,
            actor,
            session,
            cursor: Arc::new(Mutex::new(branch)),
            pending_web_receipt: Arc::new(Mutex::new(None)),
            pending_retry_operation: Arc::new(Mutex::new(None)),
            pending_retry_policy: Arc::new(Mutex::new(None)),
            #[cfg(feature = "routing")]
            pending_routing_operation: Arc::new(Mutex::new(None)),
        }
    }
    pub fn session_id(&self) -> &str {
        &self.session
    }
    pub fn branch(&self) -> Result<BranchInfo> {
        self.cursor
            .lock()
            .map(|g| g.clone())
            .map_err(|_| Error::Storage("branch cursor lock poisoned"))
    }
    pub fn view(&self) -> Result<SessionView> {
        let b = self.branch()?;
        self.store
            .activate(&self.actor, &self.session, &b.id)
            .map(|x| x.0)
    }
    pub fn store(&self) -> SessionStore {
        self.store.clone()
    }
    /// Host-only: select retry semantics for the next newly admitted logical operation.
    /// The policy is persisted by begin_sync before any provider call can occur.
    pub fn configure_next_retry(&self, policy: crate::RetryPolicy) -> Result<()> {
        policy.validate()?;
        let mut slot = self
            .pending_retry_policy
            .lock()
            .map_err(|_| Error::Storage("retry policy binding poisoned"))?;
        if slot.is_some() {
            return Err(Error::Conflict(
                "a retry policy is already bound to the next admission",
            ));
        }
        *slot = Some(policy);
        Ok(())
    }
    /// Host-only interactive/autonomous retry lineage binding. It does not itself
    /// dispatch a provider and is consumed only by the next successful begin.
    pub fn bind_retry_operation(&self, operation: impl Into<String>) -> Result<()> {
        let operation = operation.into();
        if operation.is_empty() || operation.len() > 160 {
            return Err(Error::Config("invalid retry operation ID".into()));
        }
        let mut slot = self
            .pending_retry_operation
            .lock()
            .map_err(|_| Error::Storage("retry binding poisoned"))?;
        if slot.is_some() {
            return Err(Error::Conflict(
                "a retry operation is already awaiting admission",
            ));
        }
        *slot = Some(operation);
        Ok(())
    }
    pub fn clear_retry_operation(&self) -> Result<()> {
        let mut slot = self
            .pending_retry_operation
            .lock()
            .map_err(|_| Error::Storage("retry binding poisoned"))?;
        *slot = None;
        Ok(())
    }
    /// Host-only one-shot binding from a durable Phase-40 route selection to
    /// the next working Chat admission. It never dispatches by itself.
    #[cfg(feature = "routing")]
    pub fn bind_routing_operation(&self, operation: impl Into<String>) -> Result<()> {
        let operation = operation.into();
        if operation.is_empty() || operation.len() > 160 {
            return Err(Error::Config("invalid routing operation ID".into()));
        }
        let mut slot = self
            .pending_routing_operation
            .lock()
            .map_err(|_| Error::Storage("routing binding poisoned"))?;
        if slot.is_some() {
            return Err(Error::Conflict(
                "a routing operation is already awaiting working admission",
            ));
        }
        *slot = Some(operation);
        Ok(())
    }

    /// Drop only the ephemeral next-begin binding when working admission never
    /// consumed it. The durable route operation remains inspectable.
    #[cfg(feature = "routing")]
    pub fn clear_routing_operation(&self, operation: &str) -> Result<()> {
        let mut slot = self
            .pending_routing_operation
            .lock()
            .map_err(|_| Error::Storage("routing binding poisoned"))?;
        if slot.as_deref() == Some(operation) {
            *slot = None;
        }
        Ok(())
    }

    /// Host-only causal binding. The durable accepted HTTP receipt already
    /// exists; this merely chooses it for the next begin transaction.
    pub fn bind_web_receipt(&self, receipt: u64) -> Result<()> {
        let mut slot = self
            .pending_web_receipt
            .lock()
            .map_err(|_| Error::Storage("request binding poisoned"))?;
        if slot.is_some() {
            return Err(Error::Conflict(
                "a web receipt is already awaiting native admission",
            ));
        }
        *slot = Some(receipt);
        Ok(())
    }
    /// Drop only this host's ephemeral next-begin hint after preflight rejection
    /// or panic. The durable receipt is retained; no command is replayed.
    pub fn clear_web_receipt(&self, receipt: u64) -> Result<()> {
        let mut slot = self
            .pending_web_receipt
            .lock()
            .map_err(|_| Error::Storage("request binding poisoned"))?;
        if *slot == Some(receipt) {
            *slot = None;
        }
        Ok(())
    }
    fn begin_sync(&self, start: TurnStart) -> Result<()> {
        let receipt = self
            .pending_web_receipt
            .lock()
            .map_err(|_| Error::Storage("request binding unavailable"))?
            .take();
        let retry_of = self
            .pending_retry_operation
            .lock()
            .map_err(|_| Error::Storage("retry binding unavailable"))?
            .take();
        let configured_policy = self
            .pending_retry_policy
            .lock()
            .map_err(|_| Error::Storage("retry policy binding unavailable"))?
            .take();
        #[cfg(feature = "routing")]
        let routing_operation = self
            .pending_routing_operation
            .lock()
            .map_err(|_| Error::Storage("routing binding unavailable"))?
            .take();
        #[cfg(not(feature = "routing"))]
        let routing_operation: Option<String> = None;
        if start.prompt.trim().is_empty()
            || start.prompt.len() > aw_agent_core::limits::MAX_PROMPT_BYTES
        {
            return Err(Error::Limit("invalid prompt size"));
        }
        let mut cursor = self
            .cursor
            .lock()
            .map_err(|_| Error::Storage("branch cursor unavailable"))?;
        let process = self.store.process_identity()?;
        let mut c = self.store.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let shared_submission = authorize_append_or_shared_receipt(
            &tx,
            &start.identity.actor,
            receipt,
            &self.session,
            &cursor.id,
            Some(&start.prompt),
        )?;
        let s = session_in(&tx, &self.session)?;
        let b = branch_in(&tx, &self.session, &cursor.id)?;
        let writer =
            crate::workspace_writer::require_workspace_writer_for_session(&tx, &self.session)?;
        context::authorize_node(&tx, &self.session, &b.head, &start.identity.actor)?;
        #[cfg(feature = "routing")]
        if let Some(operation) = routing_operation.as_deref() {
            crate::routing::consume_working_route_in(
                &tx,
                &start.identity.actor,
                operation,
                &self.session,
                &b,
                &start.identity,
            )?;
        }
        context::authorize_model_context(&tx, &self.session, &b, &start.identity)?;
        if b.revision != cursor.revision
            || b.head != cursor.head
            || b.active_run.is_some()
            || start.turn_id != s.next_turn
        {
            return Err(Error::Conflict(
                "session/branch changed; reload before submitting, no automatic rebase",
            ));
        }
        if start.turn_id > (1u64 << 42) - 1 {
            return Err(Error::Limit("turn ID limit reached"));
        }
        let imported: i64 = db(tx.query_row(
            "SELECT import_review_needed FROM sessions WHERE id=?1",
            [&self.session],
            |r| r.get(0),
        ))?;
        if imported != 0 {
            return Err(Error::Denied(
                "review imported history and explicitly acknowledge before execution",
            ));
        }
        let physical_root = key(&s.binding)?;
        let child_run_key = crate::subagents::model_only_child_run_key(
            &tx,
            &self.session,
            &physical_root,
            &start.identity,
        )?;
        let blocked:i64=db(tx.query_row("SELECT (SELECT count(*) FROM hazards WHERE root_key=?1 AND state<>'settled') + (SELECT count(*) FROM runs WHERE root_key=?1 AND status='running')",[&physical_root],|r|r.get(0)))?;
        if blocked > 0 && child_run_key.is_none() {
            return Err(Error::Denied(
                "execution context busy or needs inspection; resume the affected session",
            ));
        }
        let root = child_run_key.unwrap_or(physical_root);
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM nodes WHERE session_id=?1",
            [&self.session],
            |r| r.get(0),
        ))?;
        if count >= MAX_SESSION_NODES as i64 - 1 {
            return Err(Error::Limit(
                "session tree limit reached; export and start another session",
            ));
        }
        let node = id("n")?;
        let run = id("r")?;
        let txid = crate::durability::tx_id()?;
        insert_node(
            &tx,
            &self.session,
            &node,
            Some(&b.head),
            "prompt",
            &start.identity.actor,
            serde_json::json!({"turn_id":start.turn_id,"prompt":start.prompt,"identity":start.identity}),
        )?;
        context::apply::consume_pending(&tx, &start.identity.actor, &self.session, &b, &node)?;
        let (operation, attempt_no, policy) = if let Some(existing) = retry_of {
            let (session,origin_head,identity,policy_json,state):(String,String,String,String,String)=db(tx.query_row("SELECT session_id,origin_head,identity_json,retry_policy_json,state FROM llm_operations WHERE id=?1",[&existing],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))))?;
            if session != self.session || origin_head != b.head {
                return Err(Error::Conflict("same-context retry must target the original pinned head; fork from it or create a new operation"));
            }
            let old_identity: aw_agent_core::InvocationIdentity = decode(&identity)?;
            if !same_retry_destination(&old_identity, &start.identity) {
                return Err(Error::Denied(
                    "same-context retry must keep the original account/model/payer/custody/executor selection",
                ));
            }
            if routing_operation.is_some() {
                return Err(Error::Conflict(
                    "same-context retry reuses its original routing selection; a new route receipt is not allowed",
                ));
            }
            if !matches!(
                state.as_str(),
                "interrupted" | "failed" | "needs_attention" | "retry_scheduled"
            ) {
                return Err(Error::Conflict("operation is not awaiting retry"));
            }
            let policy = decode::<crate::RetryPolicy>(&policy_json)?;
            let schedule: Option<(String, Option<String>)> = db(tx
                .query_row(
                    "SELECT state,claimed_by_client FROM retry_schedules WHERE operation_id=?1",
                    [&existing],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional())?;
            match policy.mode {
                crate::RetryMode::Interactive
                    if !matches!(
                        schedule.as_ref().map(|v| v.0.as_str()),
                        Some("waiting_user")
                    ) =>
                {
                    return Err(Error::Denied(
                        "interactive retry must be explicitly authorized before admission",
                    ))
                }
                crate::RetryMode::Autonomous
                    if !matches!(schedule.as_ref().map(|v| v.0.as_str()), Some("claimed")) =>
                {
                    return Err(Error::Denied(
                        "autonomous retry must hold a durable due claim before admission",
                    ))
                }
                _ => {}
            }
            let next: u32 = db(tx.query_row(
                "SELECT coalesce(max(attempt_no),0)+1 FROM llm_attempts WHERE operation_id=?1",
                [&existing],
                |r| r.get(0),
            ))?;
            (existing, next, policy)
        } else {
            let operation = id("op")?;
            let policy = configured_policy.unwrap_or_default();
            policy.validate()?;
            let kind = if matches!(policy.mode, crate::RetryMode::Autonomous) {
                "autonomous"
            } else {
                "interactive"
            };
            db(tx.execute("INSERT INTO llm_operations(id,session_id,origin_branch_id,origin_head,origin_revision,prompt_node,actor,identity_json,retry_policy_json,kind,state,created_at,origin_writer_epoch,routing_operation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'running',?11,?12,?13)",params![operation,self.session,b.id,b.head,b.revision,node,start.identity.actor.as_str(),json(&start.identity)?,json(&policy)?,kind,now(),writer.writer_epoch,routing_operation]))?;
            (operation, 1, policy)
        };
        if attempt_no > policy.max_attempts.max(1) {
            return Err(Error::Conflict("retry policy exhausted before dispatch"));
        }
        db(tx.execute("INSERT INTO runs(id,session_id,branch_id,turn_id,prompt_node,identity_json,root_key,status,started_at,operation_id,attempt_no,produced_from_head,produced_from_revision,attach_status,produced_from_writer_epoch,owner_process_id,owner_store_generation) VALUES(?1,?2,?3,?4,?5,?6,?7,'running',?8,?9,?10,?11,?12,'pending',?13,?14,?15)",params![run,self.session,b.id,start.turn_id,node,json(&start.identity)?,root,now(),operation,attempt_no,b.head,b.revision,writer.writer_epoch,process.as_ref().map(|p|p.process_id.as_str()),process.as_ref().map(|p|p.store_generation)]))?;
        db(tx.execute("INSERT INTO llm_attempts(id,operation_id,attempt_no,run_id,dispatch_state,started_at) VALUES(?1,?2,?3,?4,'admitted',?5)",params![id("att")?,operation,attempt_no,run,now()]))?;
        db(tx.execute(
            "UPDATE llm_operations SET prompt_node=?1,state='running',settled_at=NULL WHERE id=?2",
            params![node, operation],
        ))?;
        db(tx.execute(
            "DELETE FROM retry_schedules WHERE operation_id=?1",
            [&operation],
        ))?;
        db(tx.execute("UPDATE recovery_hints SET state='resolved',resolved_at=?1 WHERE operation_id=?2 AND state='open'",params![now(),operation]))?;
        crate::durability::commit_event(
            &tx,
            &txid,
            "llm.attempt_admitted",
            "llm_operation",
            &operation,
            Some(&start.identity.actor),
            serde_json::json!({"session_id":self.session,"branch_id":b.id,"run_id":run,"attempt_no":attempt_no,"source_head":b.head,"source_revision":b.revision,"source_writer_epoch":writer.writer_epoch}),
        )?;
        if let Some(receipt) = receipt {
            let changed=db(tx.execute("UPDATE web_requests SET native_run_id=?1,owner_process_id=?2,owner_store_generation=?3 WHERE seq=?4 AND actor=?5 AND session_id=?6 AND branch_id=?7 AND state='accepted' AND native_run_id IS NULL",params![run,process.as_ref().map(|p|p.process_id.as_str()),process.as_ref().map(|p|p.store_generation),receipt,start.identity.actor.as_str(),self.session,b.id]))?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "web receipt no longer matches this actor/session/branch",
                ));
            }
        }
        let changed=db(tx.execute("UPDATE branches SET head=?1,active_run=?2,revision=revision+1 WHERE id=?3 AND head=?4 AND revision=?5 AND active_run IS NULL",params![node,run,b.id,b.head,b.revision]))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "branch changed during admission; no provider call started",
            ));
        }
        if let Some(submission) = shared_submission {
            let at = now();
            let admitted = db(tx.execute(
                "UPDATE shared_draft_submissions
                 SET status='admitted',admitted_node_id=?1,updated_at=?2
                 WHERE submission_id=?3 AND status='sealed'",
                params![node, at, submission],
            ))?;
            if admitted != 1 {
                return Err(Error::Conflict(
                    "shared submission changed before branch admission",
                ));
            }
            db(tx.execute(
                "UPDATE shared_drafts
                 SET state='admitted',updated_at=?1
                 WHERE state='sealed' AND EXISTS(
                   SELECT 1 FROM shared_draft_submissions s
                   WHERE s.submission_id=?2
                     AND s.draft_id=shared_drafts.draft_id
                     AND s.generation=shared_drafts.generation
                 )",
                params![at, submission],
            ))?;
        }
        crate::durability::ref_event(
            &tx,
            &b,
            &node,
            b.revision + 1,
            &start.identity.actor,
            "prompt_admitted",
            Some(&run),
            &txid,
        )?;
        db(tx.execute(
            "UPDATE sessions SET next_turn=next_turn+1,updated_at=?1 WHERE id=?2",
            params![now(), self.session],
        ))?;
        let next = branch_in(&tx, &self.session, &b.id)?;
        db(tx.commit())?;
        *cursor = next;
        Ok(())
    }
    fn append_sync(&self, turn_id: u64, entry: JournalEntry) -> Result<()> {
        let cursor = self.branch()?;
        let process = self.store.process_identity()?;
        let mut c = self.store.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (run, actor, root) = active_run(&tx, &self.session, &cursor.id, turn_id)?;
        authorize_append_or_shared_run(&tx, &actor, &run, &self.session)?;
        let branch = branch_in(&tx, &self.session, &cursor.id)?;
        context::authorize_node(&tx, &self.session, &branch.head, &actor)?;
        // AW-P6-10F: no stale original WriterEpoch may START another paid
        // invocation, approve a NEW tool-argument revision or START another
        // Tool/OS effect after writer failover/failback. Check atomically
        // inside the original BEGIN IMMEDIATE against the epoch pinned when
        // this exact run was first admitted. Even if the local node later
        // regains ownership, an epoch-1 run must not inherit epoch-3 rights.
        // Factual InvocationFinished/ToolFinished/Assistant receipts are NOT
        // blocked here: already attempted paid/external work must remain
        // journalable for explicit inspection, never discarded/replayed.
        if matches!(
            &entry,
            JournalEntry::InvocationStarted { .. }
                | JournalEntry::ToolStarted { .. }
                | JournalEntry::ToolRevised { .. }
        ) {
            let admitted_epoch: u64 = db(tx.query_row(
                "SELECT produced_from_writer_epoch FROM runs WHERE id=?1",
                [&run],
                |row| row.get(0),
            ))?;
            workspace_writer::verify_writer_epoch_for_session(&tx, &self.session, admitted_epoch)?;
        }
        let encoded = json(&entry)?;
        if encoded.len() > 2 * aw_agent_core::tool::MAX_TRACE_BYTES {
            return Err(Error::Limit("journal entry exceeds bound"));
        }
        let count: i64 = db(tx.query_row(
            "SELECT count(*) FROM run_events WHERE run_id=?1",
            [&run],
            |r| r.get(0),
        ))?;
        if count >= 512 {
            return Err(Error::Limit("journal event limit reached"));
        }
        // Every tool fact is checked against the exact prior Assistant and
        // durable run events. A denial is not a started tool, while an effect
        // result cannot be invented without a matching started intent.
        if matches!(
            &entry,
            JournalEntry::ToolDenied { .. }
                | JournalEntry::ToolRevised { .. }
                | JournalEntry::ToolStarted { .. }
                | JournalEntry::ToolFinished { .. }
        ) {
            let mut q =
                db(tx.prepare("SELECT payload FROM run_events WHERE run_id=?1 ORDER BY seq"))?;
            let prior = db(q.query_map([&run], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| Error::Storage("cannot load prior tool events"))?
                .into_iter()
                .map(|encoded| decode::<JournalEntry>(&encoded))
                .collect::<Result<Vec<_>>>()?;
            let proposed = |call: &aw_agent_core::ToolCall| {
                prior.iter().any(|event| {
                    matches!(
                        event,JournalEntry::Assistant{message}
                            if message.tool_calls.iter().any(|original|original==call)
                    )
                })
            };
            let revision = |id: &str| {
                prior.iter().find_map(|event| match event {
                    JournalEntry::ToolRevised {
                        original,
                        revised,
                        effect,
                        source_id,
                        source_kind,
                        source_epoch,
                    } if original.id == id => Some((
                        original,
                        revised,
                        effect,
                        source_id,
                        source_kind,
                        source_epoch,
                    )),
                    _ => None,
                })
            };
            let already = |id: &str| {
                prior.iter().any(|event| match event {
                    JournalEntry::ToolStarted { record, .. }
                    | JournalEntry::ToolFinished { record }
                    | JournalEntry::ToolDenied { record } => record.call.id == id,
                    _ => false,
                })
            };
            match &entry {
                JournalEntry::ToolDenied { record }
                    if !proposed(&record.call)
                        || already(&record.call.id)
                        || revision(&record.call.id).is_some() =>
                {
                    return Err(Error::Denied(
                        "denial must match one unstarted original assistant tool",
                    ));
                }
                JournalEntry::ToolRevised { original, .. }
                    if !proposed(original)
                        || already(&original.id)
                        || revision(&original.id).is_some() =>
                {
                    return Err(Error::Denied(
                        "revision must match one unstarted original assistant tool",
                    ));
                }
                JournalEntry::ToolStarted { record, source } => {
                    if already(&record.call.id) {
                        return Err(Error::Denied(
                            "tool effect intent already started/denied/finished",
                        ));
                    }
                    if let Some((_, revised, effect, source_id, source_kind, source_epoch)) =
                        revision(&record.call.id)
                    {
                        if revised != &record.call
                            || effect != &record.effect
                            || source.as_ref().is_some_and(|pinned| {
                                pinned.id != *source_id
                                    || pinned.kind != *source_kind
                                    || pinned.epoch != *source_epoch
                            })
                        {
                            return Err(Error::Denied(
                                "tool intent source/effect differs from approved revision",
                            ));
                        }
                    } else if !proposed(&record.call) {
                        return Err(Error::Denied(
                            "tool effect intent must match original Assistant",
                        ));
                    }
                }
                JournalEntry::ToolFinished { record } => {
                    if record.status == ToolStatus::Running || record.status == ToolStatus::Denied {
                        return Err(Error::Denied(
                            "tool receipt must settle an actually started effect",
                        ));
                    }
                    if !prior.iter().any(|event| {
                        matches!(event,
                        JournalEntry::ToolStarted{record:started, ..}
                            if started.call==record.call && started.effect==record.effect
                                && started.execution_site==record.execution_site)
                    }) || prior.iter().any(|event| {
                        matches!(event,
                            JournalEntry::ToolFinished{record:finished}
                                if finished.call.id==record.call.id)
                    }) {
                        return Err(Error::Denied(
                            "tool receipt lacks exact unique effect intent",
                        ));
                    }
                }
                _ => {}
            }
        }
        let kind = match &entry {
            JournalEntry::InvocationStarted { ordinal, identity } => {
                if identity.actor != actor || *ordinal == 0 || *ordinal > 32 {
                    return Err(Error::Denied("invocation identity/ordinal mismatch"));
                }
                context::authorize_model_context(&tx, &self.session, &branch, identity)?;
                // AW-P6-10H: original paid provider invocations are SERIAL
                // and strictly ordinal. The primary key (run_id,ordinal)
                // already bounds these rows to the immutable max 32 provider
                // attempts per Turn. One indexed aggregate inside the same
                // BEGIN IMMEDIATE checks both previous payment closure and
                // missing/overlapping ordinals, before any execution lease.
                // Only a fully closed original response or trusted adapter
                // BeforeDispatch may precede another paid invocation.
                let (last_ordinal, pending): (u32, u32) = db(tx.query_row(
                    "SELECT COALESCE(MAX(ordinal),0),
                            COUNT(*) - COUNT(CASE WHEN state='received' THEN 1 END)
                     FROM invocation_receipts WHERE run_id=?1",
                    [&run],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                ))?;
                if pending != 0 || last_ordinal.checked_add(1) != Some(*ordinal) {
                    return Err(Error::Denied(
                        "original provider ordinal overlaps, skips, or follows an uncertain charge",
                    ));
                }
                if let Some(process) = process.as_ref() {
                    coordination::acquire_execution_lease_in(
                        &tx,
                        process,
                        &provider_execution_scope(&run, *ordinal),
                        &run,
                        Duration::from_secs(30 * 60),
                    )?;
                }
                db(tx.execute("INSERT INTO invocation_receipts(run_id,ordinal,identity_json,state) VALUES(?1,?2,?3,'started')",params![run,ordinal,json(identity)?]))?;
                // A trusted no-dispatch retry may have left the prior ordinal
                // terminal. A NEW original ordinal must mark the attempt as
                // provider_started until its own factual terminal receipt.
                db(tx.execute("UPDATE llm_attempts SET dispatch_state='provider_started' WHERE run_id=?1 AND dispatch_state IN ('admitted','provider_settled')",[&run]))?;
                "invocation_started"
            }
            JournalEntry::InvocationFinished { record } => {
                // The original single Agent emits only these factual terminal
                // outcomes. An invented status or contradictory CompletionMeta
                // must not turn a possibly charged attempt into a confirmed
                // response or counterfeit a safe pre-dispatch retry. No new
                // JSONL/SQLite schema or billable effect is created here.
                match (record.outcome.as_str(), record.meta.is_some()) {
                    ("response_received", true)
                    | ("not_dispatched", false)
                    | ("stream_rejected", false)
                    | ("failed_or_cancelled", false) => {}
                    _ => {
                        return Err(Error::Denied(
                            "original provider terminal outcome and metadata conflict",
                        ))
                    }
                }
                // The original trusted adapter is the sole source of a
                // BeforeDispatch proof. Its explicit `not_dispatched` outcome
                // with NO provider metadata is a known NO-CALL terminal, not
                // a potentially charged provider response. Existing SQLite
                // v33 has only started/received/unknown states: `received`
                // here means a definitively closed, non-unknown invocation
                // (either actual response or proven no dispatch); metadata
                // remains NULL for the no-dispatch path. Do not treat an
                // arbitrary missing response as this trusted negative proof.
                let terminal_state = if record.meta.is_some()
                    || (record.meta.is_none() && record.outcome == "not_dispatched")
                {
                    "received"
                } else {
                    "unknown"
                };
                let changed=db(tx.execute("UPDATE invocation_receipts SET metadata_json=?1,state=?2 WHERE run_id=?3 AND ordinal=?4 AND identity_json=?5 AND state='started'",params![record.meta.as_ref().map(json).transpose()?,terminal_state,run,record.ordinal,json(&record.identity)?]))?;
                if changed != 1 {
                    return Err(Error::Storage("missing/mismatched invocation intent"));
                }
                if let Some(process) = process.as_ref() {
                    coordination::settle_execution_scope_in(
                        &tx,
                        process,
                        &provider_execution_scope(&run, record.ordinal),
                        &run,
                    )?;
                }
                // AW-P6-10G: a terminal InvocationFinished without a verified
                // provider response is still UNKNOWN, not a billing success.
                // Count both in-flight and already-terminal-unknown receipts;
                // otherwise one finished-but-unknown call would incorrectly
                // clear the only evidence of a possibly charged attempt.
                let unknown: i64 = db(tx.query_row(
                    "SELECT count(*) FROM invocation_receipts
                     WHERE run_id=?1 AND state IN ('started','unknown')",
                    [&run],
                    |r| r.get(0),
                ))?;
                db(tx.execute(
                    "UPDATE llm_attempts SET dispatch_state=?1 WHERE run_id=?2",
                    params![
                        if unknown == 0 {
                            "provider_settled"
                        } else {
                            "unknown"
                        },
                        run
                    ],
                ))?;
                "invocation_finished"
            }
            JournalEntry::Assistant { message } => {
                if message.role != Role::Assistant || message.provider_context.is_some() {
                    return Err(Error::Denied("invalid durable assistant record"));
                }
                "assistant"
            }
            JournalEntry::ToolDenied { record } => {
                aw_agent_core::journal::validate_denial(record)?;
                "tool_denied"
            }
            JournalEntry::ToolRevised {
                original,
                revised,
                source_id,
                source_kind,
                source_epoch,
                ..
            } => {
                aw_agent_core::journal::validate_revision(
                    original,
                    revised,
                    source_id,
                    source_kind,
                    *source_epoch,
                )?;
                "tool_revised"
            }
            JournalEntry::ToolStarted { record, source } => {
                record.call.validate_envelope()?;
                if let Some(source) = source {
                    source.validate()?;
                }
                if record.status != ToolStatus::Running {
                    return Err(Error::Storage("tool intent must be running"));
                }
                if record.effect != ToolEffect::Read {
                    db(tx.execute("UPDATE runs SET possible_effect=1 WHERE id=?1", [&run]))?;
                    db(tx.execute("INSERT INTO hazards(run_id,session_id,root_key,state,updated_at) VALUES(?1,?2,?3,'running',?4) ON CONFLICT(run_id) DO NOTHING",params![run,self.session,root,now()]))?;
                }
                "tool_started"
            }
            JournalEntry::ToolFinished { record } => {
                if let Some(output) = &record.output {
                    output.validate()?;
                }
                "tool_finished"
            }
        };
        db(tx.execute(
            "INSERT INTO run_events(run_id,kind,payload,created_at) VALUES(?1,?2,?3,?4)",
            params![run, kind, encoded, now()],
        ))?;
        db(tx.commit())?;
        Ok(())
    }
    fn finish_sync(&self, turn: Turn) -> Result<()> {
        let mut cursor = self
            .cursor
            .lock()
            .map_err(|_| Error::Storage("branch cursor unavailable"))?;
        let mut c = self.store.connection()?;
        let run = {
            let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let (run, _actor, _root) = active_run(&tx, &self.session, &cursor.id, turn.id)?;
            // Final settlement is allowed after a mid-run membership revocation;
            // it records already-attempted work, not permission for another action.
            // The result/attempt fact commits before the branch attachment CAS so a
            // crash cannot erase a paid provider result.
            persist_finish_in(&tx, &run, turn, false)?;
            db(tx.commit())?;
            run
        };
        phase34_after_result_commit_crash_hook();
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        attach_settled_run(&tx, &run)?;
        let next = branch_in(&tx, &self.session, &cursor.id)?;
        db(tx.commit())?;
        *cursor = next;
        Ok(())
    }
    fn acknowledge_sync(&self, note: String) -> Result<()> {
        if note.trim().is_empty() || note.len() > 4096 {
            return Err(Error::Config("inspection note needs 1..4096 bytes".into()));
        }
        let mut c = self.store.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        authorize(&tx, &self.session, &self.actor, true)?;
        let n: i64 = db(tx.query_row(
            "SELECT count(*) FROM runs WHERE session_id=?1 AND status='running'",
            [&self.session],
            |r| r.get(0),
        ))?;
        if n > 0 {
            return Err(Error::Conflict("a run is still active; restart/recover its owner before inspection acknowledgement"));
        }
        db(tx.execute("UPDATE hazards SET state='settled',note=?1,updated_at=?2 WHERE session_id=?3 AND state='needs_inspection'",params![note,now(),self.session]))?;
        db(tx.execute(
            "UPDATE sessions SET import_review_needed=0 WHERE id=?1",
            [&self.session],
        ))?;
        audit(
            &tx,
            &self.session,
            &self.actor,
            "effects.inspected",
            serde_json::json!({"note":note}),
        )?;
        db(tx.commit())?;
        Ok(())
    }
}
impl TurnJournal for SessionHandle {
    fn validate_context(&self) -> JournalFuture<'_, ()> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let c = this.store.connection()?;
                let expected = this.branch()?;
                let b = branch_in(&c, &this.session, &expected.id)?;
                if b.head != expected.head || b.revision != expected.revision {
                    return Err(Error::Conflict(
                        "stored context changed; reload before inference",
                    ));
                }
                authorize_append_or_current_shared_submit(&c, &this.actor, &this.session)?;
                context::authorize_node(&c, &this.session, &b.head, &this.actor)
            })
            .await
            .map_err(|_| Error::Storage("context gate worker failed"))?
        })
    }
    fn validate_context_as<'a>(
        &'a self,
        identity: &'a aw_agent_core::InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        let this = self.clone();
        let identity = identity.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                if identity.actor != this.actor {
                    return Err(Error::Denied("context caller differs from journal actor"));
                }
                let expected = this.branch()?;
                let c = this.store.connection()?;
                let b = branch_in(&c, &this.session, &expected.id)?;
                if b.head != expected.head || b.revision != expected.revision {
                    return Err(Error::Conflict(
                        "stored context changed; reload before inference",
                    ));
                }
                context::authorize_model_context(&c, &this.session, &b, &identity)
            })
            .await
            .map_err(|_| Error::Storage("context destination gate worker failed"))?
        })
    }
    fn additional_context(&self) -> JournalFuture<'_, Vec<aw_agent_core::ContextBlock>> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let c = this.store.connection()?;
                let b = branch_in(&c, &this.session, &this.branch()?.id)?;
                context::apply::pending_context(&c, &this.actor, &this.session, &b)
            })
            .await
            .map_err(|_| Error::Storage("attachment worker failed"))?
        })
    }
    fn requires_output_gate(&self) -> JournalFuture<'_, bool> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let c = this.store.connection()?;
                let b = branch_in(&c, &this.session, &this.branch()?.id)?;
                Ok(!context::dependencies(&c, &b.head)?.is_empty()
                    || !context::apply::pending_context(&c, &this.actor, &this.session, &b)?
                        .is_empty())
            })
            .await
            .map_err(|_| Error::Storage("output gate worker failed"))?
        })
    }

    fn invocation_scope(&self) -> Result<Option<aw_agent_core::InvocationScope>> {
        Ok(Some(aw_agent_core::InvocationScope {
            session_id: self.session.clone(),
            branch_id: self.branch()?.id,
        }))
    }

    fn begin<'a>(&'a self, start: TurnStart) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.begin_sync(start))
                .await
                .map_err(|_| Error::Storage("journal worker failed"))?
        })
    }
    fn append<'a>(&'a self, turn_id: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.append_sync(turn_id, entry))
                .await
                .map_err(|_| Error::Storage("journal worker failed"))?
        })
    }
    fn finish<'a>(&'a self, turn: Turn) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.finish_sync(turn))
                .await
                .map_err(|_| Error::Storage("journal worker failed"))?
        })
    }
    fn acknowledge<'a>(&'a self, note: String) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.acknowledge_sync(note))
                .await
                .map_err(|_| Error::Storage("journal worker failed"))?
        })
    }
}
fn provider_execution_scope(run: &str, ordinal: u32) -> String {
    format!("provider:{run}:invocation:{ordinal}")
}

fn active_run(
    c: &Connection,
    session: &str,
    branch: &str,
    turn: u64,
) -> Result<(String, PrincipalId, String)> {
    let row:Option<(String,String,String)>=db(c.query_row("SELECT r.id,r.identity_json,r.root_key FROM runs r JOIN branches b ON b.id=r.branch_id WHERE r.session_id=?1 AND r.branch_id=?2 AND r.turn_id=?3 AND r.status='running' AND b.active_run=r.id",params![session,branch,turn],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
    let Some((id, identity, root)) = row else {
        return Err(Error::Conflict("no matching active journal run"));
    };
    let identity: aw_agent_core::InvocationIdentity = decode(&identity)?;
    Ok((id, identity.actor, root))
}
#[cfg(test)]
fn phase34_after_result_commit_crash_hook() {
    let Ok(path) = std::env::var("AW_PHASE34_RESULT_COMMIT_READY") else {
        return;
    };
    let _ = std::fs::write(path, b"result_committed");
    std::thread::sleep(std::time::Duration::from_secs(60));
}
#[cfg(not(test))]
fn phase34_after_result_commit_crash_hook() {}

/// AW-P6-10I: the ONLY factual projection of the original immutable RunJournal
/// provider/Tool receipts, shared by both normal finalization and crash
/// recovery. This is a read-only, bounded ORIGINAL receipt scan, not a second
/// journal or an effect executor. The pre-existing run_events cap is 512.
/// Read within the same `BEGIN IMMEDIATE` as final node/attempt settlement.
struct OriginalRunReceipts {
    invocations: Vec<InvocationRecord>,
    tool_records: Vec<ToolRecord>,
    /// Actual provider Assistant messages stored in the SAME immutable run.
    /// A Turn's provider-bound context cannot invent System instructions,
    /// ToolCall proposals or assistant outcomes not found in this source.
    assistants: Vec<aw_agent_core::Message>,
    assistant_text: String,
    has_unfinished_effect: bool,
}
fn original_run_receipts(c: &Connection, run: &str) -> Result<OriginalRunReceipts> {
    let mut invocations = BTreeMap::<u32, InvocationRecord>::new();
    let mut tools = BTreeMap::<String, (usize, ToolRecord)>::new();
    let mut text = String::new();
    let mut assistants = Vec::new();
    let mut unfinished = false;
    let mut q =
        db(c.prepare("SELECT payload FROM run_events WHERE run_id=?1 ORDER BY seq LIMIT 513"))?;
    let rows = db(q.query_map([run], |r| r.get::<_, String>(0)))?;
    for (i, row) in rows.enumerate() {
        if i >= 512 {
            return Err(Error::Limit(
                "original immutable ToolJournal exceeds 512 events",
            ));
        }
        let event: JournalEntry = decode(&db(row)?)?;
        match event {
            JournalEntry::InvocationStarted { ordinal, identity } => {
                invocations.insert(
                    ordinal,
                    InvocationRecord {
                        ordinal,
                        identity,
                        meta: None,
                        outcome: "interrupted_unknown".into(),
                        usage_verified: false,
                    },
                );
            }
            JournalEntry::InvocationFinished { record } => {
                invocations.insert(record.ordinal, record);
            }
            JournalEntry::ToolDenied { record } => {
                let n = tools.len();
                tools.insert(record.call.id.clone(), (n, record));
            }
            JournalEntry::ToolRevised { .. } => {
                // Proposed edits have not executed an effect or received an
                // independent OS/Tool receipt. They cannot mint Turn results.
            }
            JournalEntry::ToolStarted { record, .. } => {
                let n = tools.len();
                tools.insert(record.call.id.clone(), (n, record));
            }
            JournalEntry::ToolFinished { record } => {
                let n = tools.get(&record.call.id).map_or(tools.len(), |x| x.0);
                tools.insert(record.call.id.clone(), (n, record));
            }
            JournalEntry::Assistant { message } => {
                if text.len().saturating_add(message.content.len())
                    <= aw_agent_core::limits::MAX_OUTPUT_BYTES
                {
                    text.push_str(&message.content);
                }
                assistants.push(message);
            }
        }
    }
    let mut tool_records = tools.into_values().collect::<Vec<_>>();
    tool_records.sort_by_key(|(original_index, _)| *original_index);
    let tool_records = tool_records
        .into_iter()
        .map(|(_, mut record)| {
            if record.status == ToolStatus::Running {
                unfinished = true;
                record.status = ToolStatus::Interrupted;
            }
            record
        })
        .collect();
    if invocations
        .values()
        .any(|record| record.outcome == "interrupted_unknown")
    {
        unfinished = true;
    }
    Ok(OriginalRunReceipts {
        invocations: invocations.into_values().collect(),
        tool_records,
        assistants,
        assistant_text: text,
        has_unfinished_effect: unfinished,
    })
}

/// AW-P6-10J: `context_messages` is only a future MODEL-CONTEXT projection.
/// It cannot elevate provider/model/Host trust by adding an unjournaled
/// System message, fabricate Assistant ToolCall authority, or forge a Tool
/// result for a call that never actually produced a factual Tool receipt.
///
/// Keep original typed Hook projections legal: successful/failing/denied
/// Tool-result *text* can differ from the factual ToolOutput content, and
/// streaming `Turn.answer` may differ from the durable Assistant fragments.
/// Thus we check the original roles/call names and complete immutable
/// Assistant proposals, NEVER the optional Hook-visible Tool-result text.
fn validate_original_context_projection(turn: &Turn, original: &OriginalRunReceipts) -> Result<()> {
    let Some((user, rest)) = turn.context_messages.split_first() else {
        // Historical empty traces and failed/recovered Turns have no retained
        // provider-bound context, and may still be kept as factual history.
        return Ok(());
    };
    if user.role != Role::User
        || user.id != turn.id * 1024
        || user.content != turn.prompt
        || !user.tool_calls.is_empty()
        || user.tool_call_id.is_some()
        || user.name.is_some()
        || user.provider_context.is_some()
    {
        return Err(Error::Denied(
            "final Turn context must begin with original User prompt",
        ));
    }
    let mut next_assistant = 0usize;
    for message in rest {
        match message.role {
            Role::Assistant => {
                let encoded = json(message)?;
                let found = if turn.outcome == Outcome::Completed {
                    // Complete Agent history cannot silently skip earlier
                    // provider ToolCall/Assistant messages and substitute a
                    // later one. They are emitted in exactly model order.
                    let matched = original
                        .assistants
                        .get(next_assistant)
                        .is_some_and(|actual| json(actual).is_ok_and(|v| v == encoded));
                    if matched {
                        next_assistant += 1;
                    }
                    matched
                } else {
                    // Failed/cancelled execution may stop after the durable
                    // Assistant event but BEFORE its context projection.
                    // Permit only a factual ordered subsequence in that case.
                    let mut matched = false;
                    while let Some(actual) = original.assistants.get(next_assistant) {
                        next_assistant += 1;
                        if json(actual)? == encoded {
                            matched = true;
                            break;
                        }
                    }
                    matched
                };
                if !found {
                    return Err(Error::Denied(
                        "final Turn Assistant/context proposal is not in immutable original journal",
                    ));
                }
            }
            Role::Tool => {
                let Some(id) = message.tool_call_id.as_deref() else {
                    return Err(Error::Denied(
                        "final Turn Tool context lacks original call ID",
                    ));
                };
                let matched = original.tool_records.iter().any(|record| {
                    record.call.id == id
                        && message.name.as_deref() == Some(record.call.name.as_str())
                        && matches!(
                            record.status,
                            ToolStatus::Succeeded | ToolStatus::Failed | ToolStatus::Denied
                        )
                        && record.output.is_some()
                });
                if !matched {
                    return Err(Error::Denied(
                        "final Turn Tool context lacks factual original Tool receipt",
                    ));
                }
            }
            Role::System | Role::User => {
                return Err(Error::Denied(
                    "final Turn cannot invent System or additional User context authority",
                ));
            }
        }
    }
    if turn.outcome == Outcome::Completed && next_assistant != original.assistants.len() {
        return Err(Error::Denied(
            "completed Turn context omits original immutable provider Assistant messages",
        ));
    }
    Ok(())
}

fn persist_finish_in(c: &Connection, run: &str, mut turn: Turn, recovered: bool) -> Result<()> {
    super::types::validate_turn(&turn)?;
    let (session,branch,prompt_node,identity,possible,operation,attempt_no,source_head,source_revision):(String,String,String,String,i64,String,u32,String,u64)=db(c.query_row("SELECT session_id,branch_id,prompt_node,identity_json,possible_effect,operation_id,attempt_no,produced_from_head,produced_from_revision FROM runs WHERE id=?1 AND status='running'",[run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))))?;
    let who: aw_agent_core::InvocationIdentity = decode(&identity)?;
    let prompt = node_in(c, &session, &prompt_node)?;
    if turn.actor.as_ref() != Some(&who.actor)
        || turn.prompt != prompt.payload["prompt"].as_str().unwrap_or("")
        || turn.model != who.model
        || turn.profile != who.account_id
    {
        return Err(Error::Storage("final turn differs from admitted intent"));
    }
    let original = original_run_receipts(c, run)?;
    validate_original_context_projection(&turn, &original)?;
    // The Turn is a PRESENTATION/REPLAY PROJECTION, not a writable source of
    // paid usage, provider identity or Tool outcome. Compare complete bounded
    // records to the immutable original started/finished/denied receipts.
    // Recovery uses this same projection, not a separately reimplemented
    // (potentially more permissive) reconstruction algorithm.
    // The top-level completion metadata is not an independent token/usage
    // ledger. When supplied, it MUST agree with one actually journaled
    // provider completion; a hook/presenter cannot invent a higher bill.
    // Absence is allowed for interrupted/recovered Turns and older factual
    // attachments whose receipt lives in the invocation records themselves.
    let metadata_factual = match turn.meta.as_ref() {
        None => true,
        Some(meta) => {
            let serialized = json(meta)?;
            original.invocations.iter().any(|record| {
                record
                    .meta
                    .as_ref()
                    .is_some_and(|actual| json(actual).is_ok_and(|fact| fact == serialized))
            })
        }
    };
    let unknown_provider_completed = turn.outcome == Outcome::Completed
        && original.invocations.iter().any(|record| {
            matches!(
                record.outcome.as_str(),
                "failed_or_cancelled" | "stream_rejected" | "interrupted_unknown"
            )
        });
    if json(&turn.invocations)? != json(&original.invocations)?
        || json(&turn.tool_records)? != json(&original.tool_records)?
        || !metadata_factual
        || unknown_provider_completed
        || (turn.outcome == Outcome::Completed && original.has_unfinished_effect)
    {
        return Err(Error::Denied(
            "final Turn provider/Tool receipts differ from immutable original journal",
        ));
    }
    turn.needs_reconciliation |= possible != 0 && (recovered || turn.outcome != Outcome::Completed);
    let node = id("n")?;
    insert_node(
        c,
        &session,
        &node,
        Some(&prompt_node),
        "turn",
        &who.actor,
        serde_json::to_value(&turn).map_err(|_| Error::Storage("invalid final turn"))?,
    )?;
    let status = if recovered {
        "interrupted"
    } else {
        match turn.outcome {
            Outcome::Completed => "completed",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
            Outcome::Incomplete => "incomplete",
        }
    };
    // This transaction deliberately leaves attach_status=pending and the branch
    // active_run in place. A later short transaction performs only the canonical
    // ref attachment CAS. Startup reconciles any durable pending attachment
    // without another provider call.
    db(c.execute(
        "UPDATE runs SET status=?1,final_node=?2,ended_at=?3 WHERE id=?4 AND status='running' AND attach_status='pending'",
        params![status, node, now(), run],
    ))?;
    // An earlier InvocationFinished(meta=None) already changed state to
    // 'unknown': it is NOT cured by the absence of an in-flight 'started'
    // row when the final Turn is attached or the owner is crash-recovered.
    let unknown: i64 = db(c.query_row(
        "SELECT count(*) FROM invocation_receipts
         WHERE run_id=?1 AND state IN ('started','unknown')",
        [run],
        |r| r.get(0),
    ))?;
    db(c.execute(
        "UPDATE invocation_receipts SET state='unknown' WHERE run_id=?1 AND state='started'",
        [run],
    ))?;
    let attempt_dispatch = if unknown > 0 {
        "unknown"
    } else {
        "provider_settled"
    };
    db(c.execute(
        "UPDATE llm_attempts SET dispatch_state=?1,outcome=?2,settled_at=?3 WHERE run_id=?4",
        params![attempt_dispatch, status, now(), run],
    ))?;
    let op_state = if recovered || unknown > 0 {
        "interrupted"
    } else if turn.outcome == Outcome::Completed {
        "completed"
    } else {
        "failed"
    };
    db(c.execute(
        "UPDATE llm_operations SET state=?1,settled_at=?2 WHERE id=?3",
        params![op_state, now(), operation],
    ))?;
    if recovered || unknown > 0 {
        let policy_json: String = db(c.query_row(
            "SELECT retry_policy_json FROM llm_operations WHERE id=?1",
            [&operation],
            |r| r.get(0),
        ))?;
        let policy: crate::RetryPolicy = decode(&policy_json)?;
        let kind = if matches!(policy.mode, crate::RetryMode::Autonomous)
            && policy.retry_unknown
            && attempt_no < policy.max_attempts
        {
            "retry_scheduled"
        } else {
            "retry_available"
        };
        let severity = if possible != 0 { "blocking" } else { "warning" };
        let message = if possible != 0 {
            "The process stopped after an external effect may have begun. Reconcile tool effects before retrying."
        } else {
            "The process stopped after an LLM call may have been dispatched. Usage may have occurred; retry creates a new paid attempt."
        };
        let hint = id("hint")?;
        db(c.execute("INSERT INTO recovery_hints(id,operation_id,session_id,branch_id,kind,severity,message,action_json,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'open',?9)",params![hint,operation,session,branch,kind,severity,message,json(&serde_json::json!({"operation_id":operation,"run_id":run,"attempt_no":attempt_no,"provider_dispatch_unknown":unknown>0,"effects_unknown":possible!=0,"origin_head":source_head,"origin_revision":source_revision,"prompt":turn.prompt,"actions":if possible!=0{serde_json::json!(["inspect_effects","leave_unresolved"])}else{serde_json::json!(["retry_same_context","retry_from_current_branch","leave_unresolved"])}}))?,now()]))?;
        if matches!(policy.mode, crate::RetryMode::Autonomous)
            && policy.retry_unknown
            && possible == 0
            && attempt_no < policy.max_attempts
        {
            let shift = attempt_no.saturating_sub(1).min(20);
            let delay = policy
                .initial_backoff_ms
                .saturating_mul(1u64 << shift)
                .min(policy.max_backoff_ms);
            let due = now().saturating_add((delay as i64).saturating_mul(1000));
            db(c.execute("INSERT INTO retry_schedules(operation_id,generation,state,due_at,next_attempt_no,updated_at) VALUES(?1,1,'scheduled',?2,?3,?4) ON CONFLICT(operation_id) DO UPDATE SET generation=generation+1,state='scheduled',due_at=excluded.due_at,next_attempt_no=excluded.next_attempt_no,claimed_by_client=NULL,claim_generation=NULL,updated_at=excluded.updated_at",params![operation,due,attempt_no+1,now()]))?;
            db(c.execute(
                "UPDATE llm_operations SET state='retry_scheduled' WHERE id=?1",
                [&operation],
            ))?;
        } else if possible == 0 {
            db(c.execute("INSERT INTO retry_schedules(operation_id,generation,state,next_attempt_no,updated_at) VALUES(?1,1,'waiting_user',?2,?3) ON CONFLICT(operation_id) DO UPDATE SET generation=generation+1,state='waiting_user',due_at=NULL,next_attempt_no=excluded.next_attempt_no,claimed_by_client=NULL,claim_generation=NULL,updated_at=excluded.updated_at",params![operation,attempt_no+1,now()]))?;
        }
    } else {
        db(c.execute(
            "DELETE FROM retry_schedules WHERE operation_id=?1",
            [&operation],
        ))?;
    }
    db(c.execute(
        "UPDATE hazards SET state=?1,updated_at=?2 WHERE run_id=?3",
        params![
            if turn.needs_reconciliation {
                "needs_inspection"
            } else {
                "settled"
            },
            now(),
            run
        ],
    ))?;
    db(c.execute(
        "UPDATE web_requests SET state=?1 WHERE native_run_id=?2 AND state='accepted'",
        params![if recovered { "interrupted" } else { "settled" }, run],
    ))?;
    db(c.execute(
        "UPDATE sessions SET updated_at=?1 WHERE id=?2",
        params![now(), session],
    ))?;
    let txid = crate::durability::tx_id()?;
    // Once the provider outcome/unknown state is durably represented in the
    // operation ledger, the process-coordination lease no longer owns replay
    // safety. Releasing it here never turns an unknown call into a retry; retry
    // authority remains the explicit/persisted operation policy above.
    coordination::reconcile_provider_run_leases(c, run)?;
    crate::durability::commit_event(
        c,
        &txid,
        "llm.attempt_settled",
        "llm_operation",
        &operation,
        Some(&who.actor),
        serde_json::json!({"session_id":session,"branch_id":branch,"run_id":run,"attempt_no":attempt_no,"outcome":status,"dispatch_unknown":unknown>0,"attach_status":"pending"}),
    )?;
    Ok(())
}

fn attach_settled_run(c: &Connection, run: &str) -> Result<bool> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        u32,
        String,
        u64,
        u64,
        String,
        String,
    )> = db(c
        .query_row(
            "SELECT session_id,branch_id,prompt_node,identity_json,operation_id,attempt_no,
                produced_from_head,produced_from_revision,produced_from_writer_epoch,
                final_node,attach_status
         FROM runs WHERE id=?1 AND status<>'running'",
            [run],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                ))
            },
        )
        .optional())?;
    let Some((
        session,
        branch,
        prompt_node,
        identity,
        operation,
        attempt_no,
        source_head,
        source_revision,
        source_writer_epoch,
        node,
        attach_status,
    )) = row
    else {
        return Err(Error::Conflict("settled run is unavailable for attachment"));
    };
    if attach_status == "attached" {
        return Ok(true);
    }
    if attach_status != "pending" {
        return Ok(false);
    }
    let who: aw_agent_core::InvocationIdentity = decode(&identity)?;
    let branch_before = branch_in(c, &session, &branch)?;
    let writer_matches =
        crate::workspace_writer::verify_writer_epoch_for_session(c, &session, source_writer_epoch)
            .is_ok();
    let txid = crate::durability::tx_id()?;
    let attached = if writer_matches
        && branch_before.active_run.as_deref() == Some(run)
        && branch_before.head == prompt_node
    {
        let changed=db(c.execute("UPDATE branches SET head=?1,revision=revision+1,active_run=NULL WHERE id=?2 AND active_run=?3 AND head=?4 AND revision=?5",params![node,branch,run,prompt_node,branch_before.revision]))?;
        if changed == 1 {
            let run_changed = db(c.execute(
                "UPDATE runs SET attach_status='attached' WHERE id=?1 AND attach_status='pending'",
                [run],
            ))?;
            if run_changed != 1 {
                return Err(Error::Conflict("run attachment state changed during CAS"));
            }
            crate::durability::ref_event(
                c,
                &branch_before,
                &node,
                branch_before.revision + 1,
                &who.actor,
                "run_settled",
                Some(run),
                &txid,
            )?;
            true
        } else {
            false
        }
    } else {
        false
    };
    if !attached {
        let changed = db(c.execute(
            "UPDATE runs SET attach_status='conflict' WHERE id=?1 AND attach_status='pending'",
            [run],
        ))?;
        if changed != 1 {
            return Ok(false);
        }
        db(c.execute(
            "UPDATE branches SET active_run=NULL WHERE id=?1 AND active_run=?2",
            params![branch, run],
        ))?;
        let hint = id("hint")?;
        db(c.execute("INSERT INTO recovery_hints(id,operation_id,session_id,branch_id,kind,severity,message,action_json,state,created_at) VALUES(?1,?2,?3,?4,'attachment_conflict','warning','A completed model result could not attach because the branch moved. The paid result is preserved; do not regenerate automatically.',?5,'open',?6)",params![hint,operation,session,branch,json(&serde_json::json!({"operation_id":operation,"run_id":run,"result_node":node,"produced_from_head":source_head,"produced_from_revision":source_revision,"produced_from_writer_epoch":source_writer_epoch,"writer_epoch_still_current":writer_matches,"actions":["fork_from_original","compare","summarize","leave_unattached"]}))?,now()]))?;
    }
    crate::durability::commit_event(
        c,
        &txid,
        if attached {
            "llm.result_attached"
        } else {
            "llm.result_attachment_conflict"
        },
        "llm_operation",
        &operation,
        Some(&who.actor),
        serde_json::json!({"session_id":session,"branch_id":branch,"run_id":run,"attempt_no":attempt_no,"result_node":node,"attach_status":if attached{"attached"}else{"conflict"}}),
    )?;
    Ok(attached)
}

impl SessionStore {
    /// Run only after exclusive process ownership is acquired. Reconstruction
    /// never calls a model/tool, never guesses a successful result, and preserves
    /// all durable model attempts and possible effect hazards.
    pub(crate) fn recover(&self) -> Result<()> {
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        self.recover_subagent_execution_bindings_after_owner_restart(&tx)?;
        self.recover_subagents_after_owner_restart(&tx)?;
        #[cfg(feature = "pipelines")]
        self.recover_teams_after_owner_restart(&tx)?;
        #[cfg(all(feature = "pipelines", feature = "routing"))]
        self.recover_planners_after_owner_restart(&tx)?;
        #[cfg(all(feature = "pipelines", feature = "routing"))]
        self.recover_reviews_after_owner_restart(&tx)?;
        let runs = {
            let direct = self.trusted_local_direct();
            if direct {
                let unowned: i64 = db(tx.query_row(
                    "SELECT count(*) FROM runs WHERE status='running' AND (owner_process_id IS NULL OR owner_store_generation IS NULL)",
                    [],
                    |r| r.get(0),
                ))?;
                if unowned != 0 {
                    return Err(Error::Conflict(
                        "direct mode found a running pre-v20/unowned run; reopen once in owner mode for authoritative recovery",
                    ));
                }
            }
            let sql = if direct {
                "SELECT r.id,r.session_id,r.turn_id,r.prompt_node,r.identity_json,r.possible_effect
                 FROM runs r
                 LEFT JOIN local_process_incarnations p ON p.process_id=r.owner_process_id
                 WHERE r.status='running'
                   AND r.owner_process_id IS NOT NULL
                   AND (p.process_id IS NULL OR p.state<>'active'
                        OR p.store_generation<>r.owner_store_generation
                        OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1))
                 ORDER BY r.started_at"
            } else {
                "SELECT r.id,r.session_id,r.turn_id,r.prompt_node,r.identity_json,r.possible_effect
                 FROM runs r
                 LEFT JOIN local_process_incarnations p ON p.process_id=r.owner_process_id
                 WHERE r.status='running'
                   AND (r.owner_process_id IS NULL OR p.process_id IS NULL OR p.state<>'active'
                        OR p.store_generation<>r.owner_store_generation
                        OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1))
                 ORDER BY r.started_at"
            };
            let mut q = db(tx.prepare(sql))?;
            let map = |r: &rusqlite::Row<'_>| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            };
            let rows = db(q.query_map([now()], map))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        for (run, session, turn_id, prompt_node, identity, possible) in runs {
            let identity: aw_agent_core::InvocationIdentity = decode(&identity)?;
            let prompt = node_in(&tx, &session, &prompt_node)?.payload["prompt"]
                .as_str()
                .ok_or(Error::Storage("missing admitted prompt"))?
                .to_owned();
            let original = original_run_receipts(&tx, &run)?;
            let turn=Turn{id:turn_id,prompt,answer:original.assistant_text,model:identity.model.clone(),profile:identity.account_id.clone(),outcome:Outcome::Failed,
                meta:None,error:Some("owner stopped before final settlement; recorded intent recovered, no action replayed".into()),actor:Some(identity.actor.clone()),tool_records:original.tool_records,
                invocations:original.invocations,context_messages:Vec::new(),needs_reconciliation:possible!=0};
            persist_finish_in(&tx, &run, turn, true)?;
            attach_settled_run(&tx, &run)?;
            audit(
                &tx,
                &session,
                &identity.actor,
                "run.recovered",
                serde_json::json!({"run_id":run,"effects_unknown":possible!=0}),
            )?;
        }
        let pending_attachments = {
            let mut q = db(tx.prepare(
                "SELECT id FROM runs WHERE status<>'running' AND attach_status='pending' ORDER BY ended_at,id",
            ))?;
            let rows = db(q.query_map([], |r| r.get::<_, String>(0)))?;
            db(rows.collect::<rusqlite::Result<Vec<_>>>())?
        };
        for run in pending_attachments {
            attach_settled_run(&tx, &run)?;
        }
        // Phase 40 routing classifiers are paid external effects separate from
        // Chat turns. A dead/fenced owner can only convert a started attempt to
        // an inspectable unknown state; recovery never creates another attempt.
        let routing_at = now();
        let routing_where = if self.trusted_local_direct() {
            "state='started' AND EXISTS(
               SELECT 1 FROM local_process_incarnations p
               WHERE p.process_id=routing_attempts.owner_process_id
                 AND (p.state<>'active' OR p.store_generation<>routing_attempts.owner_store_generation
                      OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1)))"
        } else {
            "state='started' AND (
               NOT EXISTS(SELECT 1 FROM local_process_incarnations p WHERE p.process_id=routing_attempts.owner_process_id)
               OR EXISTS(
                 SELECT 1 FROM local_process_incarnations p
                 WHERE p.process_id=routing_attempts.owner_process_id
                   AND (p.state<>'active' OR p.store_generation<>routing_attempts.owner_store_generation
                        OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1))))"
        };
        db(tx.execute(
            &format!(
                "UPDATE routing_attempts
                 SET state='unknown_external_effect',
                     error='owning process stopped after routing classifier intent; usage may be unknown; no replay',
                     settled_at=?1
                 WHERE {routing_where}"
            ),
            [routing_at],
        ))?;
        db(tx.execute(
            "UPDATE routing_operations
             SET state='unknown_external_effect',
                 error='routing classifier owner stopped after dispatch intent; usage may be unknown; no replay',
                 updated_at=?1
             WHERE state='classifier_started' AND EXISTS(
               SELECT 1 FROM routing_attempts a
               WHERE a.operation_id=routing_operations.id AND a.state='unknown_external_effect')",
            [routing_at],
        ))?;

        if self.trusted_local_direct() {
            let unowned_context: i64 = db(tx.query_row(
                "SELECT count(*) FROM context_operations WHERE state='started' AND (owner_process_id IS NULL OR owner_store_generation IS NULL)",
                [],
                |r| r.get(0),
            ))?;
            let unowned_web: i64 = db(tx.query_row(
                "SELECT count(*) FROM web_requests WHERE state='accepted' AND (owner_process_id IS NULL OR owner_store_generation IS NULL)",
                [],
                |r| r.get(0),
            ))?;
            if unowned_context != 0 || unowned_web != 0 {
                return Err(Error::Conflict(
                    "direct mode found unowned in-progress context/web state; reopen once in owner mode for authoritative recovery",
                ));
            }
            let at = now();
            db(tx.execute(
                "UPDATE context_operations
                 SET state='interrupted',error='owning local process stopped; usage may be unknown; no replay',settled_at=?1
                 WHERE state='started' AND EXISTS(
                   SELECT 1 FROM local_process_incarnations p
                   WHERE p.process_id=context_operations.owner_process_id
                     AND (p.state<>'active' OR p.store_generation<>context_operations.owner_store_generation
                          OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1)))",
                [at],
            ))?;
            db(tx.execute(
                "UPDATE web_requests SET state='interrupted'
                 WHERE state='accepted' AND EXISTS(
                   SELECT 1 FROM local_process_incarnations p
                   WHERE p.process_id=web_requests.owner_process_id
                     AND (p.state<>'active' OR p.store_generation<>web_requests.owner_store_generation
                          OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1)))",
                [at],
            ))?;
        } else {
            let at = now();
            db(tx.execute(
                "UPDATE context_operations
                 SET state='interrupted',error='owning local process stopped; usage may be unknown; no replay',settled_at=?1
                 WHERE state='started' AND (
                   owner_process_id IS NULL OR EXISTS(
                     SELECT 1 FROM local_process_incarnations p
                     WHERE p.process_id=context_operations.owner_process_id
                       AND (p.state<>'active' OR p.store_generation<>context_operations.owner_store_generation
                            OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1))))",
                [at],
            ))?;
            db(tx.execute(
                "UPDATE web_requests SET state='interrupted'
                 WHERE state='accepted' AND (
                   owner_process_id IS NULL OR EXISTS(
                     SELECT 1 FROM local_process_incarnations p
                     WHERE p.process_id=web_requests.owner_process_id
                       AND (p.state<>'active' OR p.store_generation<>web_requests.owner_store_generation
                            OR (p.lease_expires_at IS NOT NULL AND p.lease_expires_at<=?1))))",
                [at],
            ))?;
        }
        db(tx.commit())?;
        Ok(())
    }
    /// Durable browser deduplication. The request actor/target is supplied by the
    /// authenticated host. Acceptance is not a completed model response.
    pub fn reserve_web(
        &self,
        actor: &PrincipalId,
        request: &str,
        fingerprint: &str,
        session: &str,
        branch: &str,
        revision: u64,
    ) -> Result<WebReceipt> {
        if request.is_empty()
            || request.len() > 64
            || !request
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(Error::Config("invalid request ID".into()));
        }
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|x| x.is_ascii_hexdigit()) {
            return Err(Error::Config("invalid request fingerprint".into()));
        }
        let process = self.process_identity()?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let _shared_submission =
            authorize_append_or_shared_request(&tx, actor, request, session, branch)?;
        let prior:Option<(String,WebReceipt)>=db(tx.query_row("SELECT fingerprint,seq,view_revision,state,session_id,branch_id,native_run_id FROM web_requests WHERE actor=?1 AND request_id=?2",params![actor.as_str(),request],|r|Ok((r.get(0)?,WebReceipt{run_id:r.get(1)?,revision:r.get(2)?,state:r.get(3)?,session_id:r.get(4)?,branch_id:r.get(5)?,native_run_id:r.get(6)?}))).optional())?;
        if let Some((digest, receipt)) = prior {
            if digest == fingerprint {
                return Ok(receipt);
            }
            return Err(Error::Conflict(
                "request ID already belongs to another exact request",
            ));
        }
        let b = branch_in(&tx, session, branch)?;
        if b.active_run.is_some() {
            return Err(Error::Conflict("branch busy"));
        }
        let count: i64 = db(tx.query_row("SELECT count(*) FROM web_requests", [], |r| r.get(0)))?;
        if count >= 100_000 {
            return Err(Error::Limit(
                "durable request limit; archive/rotate the store explicitly",
            ));
        }
        db(tx.execute("INSERT INTO web_requests(actor,request_id,fingerprint,session_id,branch_id,view_revision,state,created_at,owner_process_id,owner_store_generation) VALUES(?1,?2,?3,?4,?5,?6,'accepted',?7,?8,?9)",params![actor.as_str(),request,fingerprint,session,b.id,revision,now(),process.as_ref().map(|p|p.process_id.as_str()),process.as_ref().map(|p|p.store_generation)]))?;
        let receipt = WebReceipt {
            run_id: tx.last_insert_rowid() as u64,
            revision,
            state: "accepted".into(),
            session_id: session.into(),
            branch_id: b.id,
            native_run_id: None,
        };
        db(tx.commit())?;
        Ok(receipt)
    }
    pub fn settle_web(&self, actor: &PrincipalId, run_id: u64) -> Result<()> {
        let c = self.connection()?;
        db(c.execute("UPDATE web_requests SET state='settled' WHERE actor=?1 AND seq=?2 AND state='accepted' AND native_run_id IS NULL",params![actor.as_str(),run_id]))?;
        Ok(())
    }
    pub fn lookup_web(
        &self,
        actor: &PrincipalId,
        request: &str,
        fingerprint: &str,
    ) -> Result<Option<WebReceipt>> {
        let c = self.connection()?;
        let row:Option<(String,WebReceipt)>=db(c.query_row("SELECT fingerprint,seq,view_revision,state,session_id,branch_id,native_run_id FROM web_requests WHERE actor=?1 AND request_id=?2",params![actor.as_str(),request],|r|Ok((r.get(0)?,WebReceipt{run_id:r.get(1)?,revision:r.get(2)?,state:r.get(3)?,session_id:r.get(4)?,branch_id:r.get(5)?,native_run_id:r.get(6)?}))).optional())?;
        match row {
            Some((f, r)) => {
                context::authorize_tree(&c, &r.session_id, actor)?;
                if f != fingerprint {
                    return Err(Error::Conflict("request ID collision"));
                }
                Ok(Some(r))
            }
            None => Ok(None),
        }
    }
    /// Read the exact settled result linked to an authenticated transport receipt.
    /// Current session access is rechecked. This never activates a branch, loads
    /// a provider, or reconstructs a result from a similarly worded later prompt.
    pub fn web_result(
        &self,
        actor: &PrincipalId,
        request: &str,
        fingerprint: &str,
    ) -> Result<Option<(WebReceipt, Option<Turn>)>> {
        let Some(receipt) = self.lookup_web(actor, request, fingerprint)? else {
            return Ok(None);
        };
        let c = self.connection()?;
        context::authorize_tree(&c, &receipt.session_id, actor)?;
        let text: Option<String> = if let Some(run) = &receipt.native_run_id {
            db(c.query_row("SELECT n.payload FROM runs r JOIN nodes n ON n.id=r.final_node AND n.session_id=r.session_id WHERE r.id=?1 AND r.session_id=?2 AND r.branch_id=?3 AND n.kind='turn'",params![run,&receipt.session_id,&receipt.branch_id],|r|r.get(0)).optional())?
        } else {
            None
        };
        let turn: Option<Turn> = text.as_deref().map(decode).transpose()?;
        if let Some(t) = &turn {
            super::types::validate_turn(t)?;
        }
        Ok(Some((receipt, turn)))
    }
}

#[cfg(test)]
mod phase40_retry_destination_tests {
    use super::*;
    use aw_agent_core::{AccountOwner, BillingOwner, CredentialCustody};

    fn identity(
        account: &str,
        model: &str,
        grant_revision: u64,
    ) -> aw_agent_core::InvocationIdentity {
        let actor = PrincipalId::new("unix:1000").unwrap();
        aw_agent_core::InvocationIdentity {
            actor: actor.clone(),
            account_id: account.into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor),
            credential_custody: CredentialCustody::UserDevice("local".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: model.into(),
            grant_revision,
        }
    }

    #[test]
    fn same_context_retry_allows_fresh_lease_revision_but_never_reroutes() {
        let original = identity("fast", "fast-model", 1);
        let fresh_lease = identity("fast", "fast-model", 99);
        assert!(same_retry_destination(&original, &fresh_lease));
        assert!(!same_retry_destination(
            &original,
            &identity("deep", "fast-model", 99)
        ));
        assert!(!same_retry_destination(
            &original,
            &identity("fast", "deep-model", 99)
        ));
    }
}
