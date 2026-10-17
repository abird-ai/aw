//! The workspace's single native agent owner. It runs on a dedicated thread;
//! views observe a bounded coalesced projection, not another agent implementation.
use crate::{runtime, Host};
use aw::{AgentEvent, CancellationToken, Error, Result};
use aw_agent_core::Turn;
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc, Mutex},
    thread,
};
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct SavedBranch {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SavedSubagent {
    pub parent_session: String,
    pub child_session: String,
    pub state: String,
    pub mode: String,
    pub tool_mode: String,
    pub depth: u8,
}

#[derive(Clone, Debug)]
pub struct SavedDelegationBudget {
    pub state: String,
    pub children_created: u32,
    pub max_children_created: u32,
    pub active_children: u32,
    pub max_active_children: u32,
    pub model_attempts_started: u32,
    pub max_model_attempts: u32,
}

#[derive(Clone, Debug)]
pub struct SavedSession {
    pub id: String,
    pub title: String,
    pub directory: String,
    pub autonomous: bool,
    pub archived: bool,
    pub updated_at: i64,
    pub turn_count: u64,
    pub branches: Vec<SavedBranch>,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub generation: u64,
    pub busy: bool,
    pub state_pending: bool,
    pub fatal: bool,
    pub store_id: Option<String>,
    pub session: Option<String>,
    pub branch_id: Option<String>,
    pub session_title: Option<String>,
    pub branch: String,
    pub model: String,
    pub profile: String,
    pub provider: String,
    pub effort: String,
    pub actor: String,
    pub root: String,
    pub tools: String,
    pub needs_inspection: bool,
    pub turns: Arc<Vec<Turn>>,
    pub pending_prompt: String,
    pub live: String,
    pub thinking: bool,
    pub turn_started: Option<std::time::Instant>,
    pub live_tools: Vec<aw_agent_core::ToolRecord>,
    pub rejected_prompt: String,
    pub rejected_revision: u64,
    pub activity: VecDeque<String>,
    /// P6-8C opt-in factual, ephemeral v2 UI-only EventRail, never Agent
    /// messages, paid receipts, or a second durable Session event log.
    pub event_rail:Option<aw_ui_extension::status_events::EventRail>,
    pub sessions: Vec<SavedSession>,
    pub subagents: Vec<SavedSubagent>,
    pub delegation_budget: Option<SavedDelegationBudget>,
    pub tree: String,
    pub context_output: String,
    pub context_blocks: Vec<aw_agent_core::ContextBlock>,
    pub loaded_resources: crate::tui::agent_resources::LoadedResourceSummary,
    pub status: String,
    pub auxiliary_output: Option<String>,
    pub performance: Option<crate::tui::performance::PerformanceStats>,
    pub session_tps: Option<f64>,
}
impl Snapshot {
    fn new(host: &Host) -> Self {
        Self {
            generation: 0,
            busy: false,
            state_pending: false,
            fatal: false,
            store_id: None,
            session: None,
            branch_id: None,
            session_title: None,
            branch: "main".into(),
            model: host
                .settings
                .profile
                .model
                .clone()
                .unwrap_or_else(|| "not configured".into()),
            profile: host.settings.profile_name.clone(),
            provider: host.settings.profile.kind.label().into(),
            effort: host
                .settings
                .profile
                .reasoning_effort
                .map_or_else(String::new, |effort| effort.label().into()),
            actor: format!(
                "unix:{}",
                host.context
                    .effective_uid
                    .map_or("unknown".into(), |n| n.to_string())
            ),
            root: host.tools.root.display().to_string(),
            tools: host.tools.mode.to_string(),
            needs_inspection: false,
            turns: Arc::new(vec![]),
            pending_prompt: String::new(),
            live: String::new(),
            thinking: false,
            turn_started: None,
            live_tools: Vec::new(),
            rejected_prompt: String::new(),
            rejected_revision: 0,
            activity: VecDeque::new(),
            event_rail:None,
            sessions: vec![],
            subagents: vec![],
            delegation_budget: None,
            tree: String::new(),
            context_output: String::new(),
            context_blocks: Vec::new(),
            loaded_resources: host.loaded_resources.clone(),
            status: "Workspace opened without a new model request.".into(),
            auxiliary_output: None,
            performance: None,
            session_tps: None,
        }
    }
    fn activity(&mut self, line: String) {
        while self.activity.len() >= 256 {
            self.activity.pop_front();
        }
        self.activity.push_back(line);
    }
}
#[derive(Clone)]
pub struct Projection {
    pub data: Arc<Mutex<Snapshot>>,
    tick: watch::Sender<u64>,
    cancel: Arc<Mutex<Option<CancellationToken>>>,
    branch_reader: Arc<Mutex<Option<super::branch_view::Reader>>>,
}
impl Projection {
    pub fn snapshot(&self) -> Snapshot {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn change(&self, f: impl FnOnce(&mut Snapshot)) {
        let mut s = self.data.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut s);
        s.generation = s.generation.wrapping_add(1);
        self.tick.send_replace(s.generation);
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.tick.subscribe()
    }
    pub fn cancel(&self) {
        if let Some(c) = self
            .cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            c.cancel();
        }
    }
    fn set_cancel(&self, c: Option<CancellationToken>) {
        *self.cancel.lock().unwrap_or_else(|e| e.into_inner()) = c;
    }
}
#[derive(Debug)]
pub enum Command {
    Prompt(String),
    Context(aw::context_flow::Command),
    Refresh,
    Tree,
    Resume {
        session: String,
        branch: String,
    },
    Switch(String),
    New,
    Title(String),
    SetSessionsArchived { sessions: Vec<String>, archived: bool },
    Fork {
        name: String,
        at: Option<String>,
    },
    Checkpoint(String),
    Model(String),
    Think(aw::config::ReasoningEffort),
    Side(String),
    GenerateTitle,
    PersistUiWake,
    Acknowledge(String),
    GitIndex {
        repo: aw_git::Repository,
        review: aw_git::Review,
        action: aw_git::IndexAction,
    },
    Stop,
}
pub struct Worker {
    pub projection: Projection,
    tx: mpsc::SyncSender<Command>,
    pending_ui: Arc<Mutex<Option<aw::config::UiConfig>>>,
    thread: Option<thread::JoinHandle<Host>>,
}
fn apply_pending_ui(
    host: &mut Host,
    view: &Projection,
    pending_ui: &Arc<Mutex<Option<aw::config::UiConfig>>>,
) {
    let next = pending_ui
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(ui) = next {
        if let Err(error) = host.apply_ui(ui) {
            view.change(|snapshot| {
                snapshot.status = crate::terminal::safe_text(&error.to_string());
                snapshot.activity(format!("error: {}", snapshot.status));
            });
        }
    }
}

impl Worker {
    pub fn start(host: Host) -> Result<Self> {
        let branch_reader = host
            .persistent
            .as_ref()
            .map(|p| super::branch_view::Reader::new(p.store.clone(), p.actor.clone()))
            .transpose()?;
        // Move the settled Host including its existing runtime. Dropping the
        // runtime here would invalidate pooled provider I/O; mode changes must
        // not replace the agent, store or its idle runtime resources.
        let (tx, rx) = mpsc::sync_channel::<Command>(8);
        let (tick, _) = watch::channel(0);
        let projection = Projection {
            data: Arc::new(Mutex::new(Snapshot::new(&host))),
            tick,
            cancel: Arc::new(Mutex::new(None)),
            branch_reader: Arc::new(Mutex::new(branch_reader)),
        };
        let view = projection.clone();
        let pending_ui = Arc::new(Mutex::new(None));
        let owner_pending_ui = pending_ui.clone();
        let worker=thread::Builder::new().name("aw-workspace-owner".into()).spawn(move||{
            let mut host=host;
            refresh(&mut host,&view,false);
            while let Ok(command)=rx.recv(){
                if matches!(command,Command::Stop){
                    apply_pending_ui(&mut host, &view, &owner_pending_ui);
                    break;
                }
                if matches!(command, Command::PersistUiWake) {
                    apply_pending_ui(&mut host, &view, &owner_pending_ui);
                    continue;
                }
                let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||execute(&mut host,&view,command)));
                match outcome {
                    Ok(Ok(()))=>{},
                    Ok(Err(e))=>view.change(|s|{s.status=crate::terminal::safe_text(&e.to_string());s.activity(format!("error: {}",s.status));}),
                    Err(_)=>{view.change(|s|{s.status="Native owner panicked; restart/recover and inspect effects before continuing.".into();s.needs_inspection=true;s.fatal=true;});
                        // Do not continue using a partially unwound Agent.
                        view.change(|s|{s.busy=false;s.state_pending=false;});break;}
                }
                apply_pending_ui(&mut host, &view, &owner_pending_ui);
                view.set_cancel(None);refresh(&mut host,&view,false);view.change(|s|{s.busy=false;s.state_pending=false;s.turn_started=None;});
            }
            view.set_cancel(None);host
        })?;
        Ok(Self {
            projection,
            tx,
            pending_ui,
            thread: Some(worker),
        })
    }
    pub fn persist_ui(&self, ui: aw::config::UiConfig) -> Result<()> {
        if self.projection.snapshot().fatal {
            return Err(Error::Storage("workspace owner failed; exit and recover"));
        }
        *self
            .pending_ui
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ui);
        match self.tx.try_send(Command::PersistUiWake) {
            Ok(()) | Err(mpsc::TrySendError::Full(Command::PersistUiWake)) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(Error::Conflict(
                "workspace owner unavailable for UI persistence",
            )),
        }
    }

    pub(super) fn branch_reader(&self) -> Option<super::branch_view::Reader> {
        self.projection
            .branch_reader
            .lock()
            .ok()
            .and_then(|reader| reader.clone())
    }

    pub fn send(&self, command: Command) -> Result<()> {
        let s = self.projection.snapshot();
        if s.fatal {
            return Err(Error::Storage("workspace owner failed; exit and recover"));
        }
        if s.busy || s.state_pending {
            return Err(Error::Conflict(
                "workspace owner busy; your draft is preserved",
            ));
        }
        if matches!(&command, Command::Prompt(_) | Command::Context(_)) {
            self.projection.set_cancel(Some(CancellationToken::new()));
        }
        self.projection.change(|s| {
            s.state_pending = true;
        });
        match self.tx.try_send(command) {
            Ok(()) => Ok(()),
            Err(_) => {
                self.projection.set_cancel(None);
                self.projection.change(|s| s.state_pending = false);
                Err(Error::Conflict("workspace command queue unavailable"))
            }
        }
    }
    /// Called outside any async runtime after the terminal has been restored.
    /// Cancellation is cooperative; trusted native code can still block.
    pub fn finish(mut self) -> Result<Host> {
        self.projection.cancel();
        let _ = self.tx.send(Command::Stop);
        self.thread
            .take()
            .ok_or(Error::Storage("workspace owner missing"))?
            .join()
            .map_err(|_| Error::Storage("workspace owner thread panicked"))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.projection.cancel();
        let _ = self.tx.try_send(Command::Stop);
    }
}
fn saved_workspace_sessions(
    store: &aw_store::SessionStore,
    actor: &aw_agent_core::PrincipalId,
) -> Result<(
    Vec<SavedSession>,
    Vec<SavedSubagent>,
    super::branch_view::Reader,
)> {
    let visible = store
        .list(actor)?
        .into_iter()
        .map(|session| (session, false));
    let archived = store
        .list_archived(actor)?
        .into_iter()
        .map(|session| (session, true));
    let sessions = visible
        .chain(archived)
        .take(512)
        .map(|(session, archived)| {
            let branches = store
                .branch_navigation(actor, &session.id)?
                .into_iter()
                .take(128)
                .map(|branch| SavedBranch {
                    id: branch.id,
                    name: branch.name,
                    parent: branch.parent_name,
                })
                .collect();
            let autonomous = store.session_is_autonomous(actor, &session.id)?;
            Ok(SavedSession {
                id: session.id,
                title: session.title,
                directory: session.binding.working_directory.display().to_string(),
                autonomous,
                archived,
                updated_at: session.updated_at,
                turn_count: session.next_turn.saturating_sub(1),
                branches,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let subagents = store
        .visible_subagents(actor)?
        .into_iter()
        .map(|item| SavedSubagent {
            parent_session: item.parent_session_id,
            child_session: item.child_session_id,
            state: item.state,
            mode: match item.mode {
                aw_store::SubagentMode::Foreground => "foreground".into(),
                aw_store::SubagentMode::Background => "background".into(),
            },
            tool_mode: item.tool_mode,
            depth: item.depth.unwrap_or(1),
        })
        .collect();
    let reader = super::branch_view::Reader::new(store.clone(), actor.clone())?;
    Ok((sessions, subagents, reader))
}

fn refresh(host: &mut Host, view: &Projection, with_sessions: bool) {
    let current = host.persistent.as_ref().map(|p| p.view());
    let source: Result<Option<(aw_store::SessionStore, aw_agent_core::PrincipalId)>> = (|| {
        if host.store_backend != crate::resources_cli::SessionStoreBackendArg::Sqlite {
            return Ok(None);
        }
        if host.persistent.is_none() && host.sqlite_store.is_none() {
            let Some(path) = host.state_dir.as_ref() else {
                return Ok(None);
            };
            // Do not initialize missing State merely to browse a saved view.
            if !path.join("sessions.sqlite3").try_exists()? {
                return Ok(None);
            }
        }
        let store = host.sqlite_store()?;
        let actor = match &host.persistent {
            Some(saved) => saved.actor.clone(),
            None => aw::sessions::actor(&host.context, false)?,
        };
        Ok(Some((store, actor)))
    })();
    let current_budget = match (&host.persistent, &current) {
        (Some(persistent), Some(Ok(current))) => persistent
            .store
            .delegation_root_for_session(&persistent.actor, &current.session.id)
            .ok()
            .flatten()
            .map(|root| SavedDelegationBudget {
                state: root.state,
                children_created: root.children_created,
                max_children_created: root.limits.max_children_created,
                active_children: root.active_children,
                max_active_children: root.limits.max_active_children,
                model_attempts_started: root.model_attempts_started,
                max_model_attempts: root.limits.max_model_attempts,
            }),
        _ => None,
    };
    let mut session_projection = None;
    let reader = match source {
        Ok(Some((store, actor))) => {
            if with_sessions {
                match saved_workspace_sessions(&store, &actor) {
                    Ok((sessions, subagents, reader)) => {
                        session_projection = Some((sessions, subagents));
                        Some(reader)
                    }
                    Err(error) => {
                        session_projection = Some((vec![], vec![]));
                        view.change(|snapshot| snapshot.status = error.to_string());
                        None
                    }
                }
            } else {
                super::branch_view::Reader::new(store, actor).ok()
            }
        }
        Ok(None) => {
            if with_sessions {
                session_projection = Some((vec![], vec![]));
            }
            None
        }
        Err(error) => {
            if with_sessions {
                session_projection = Some((vec![], vec![]));
            }
            view.change(|snapshot| snapshot.status = error.to_string());
            None
        }
    };
    let store_id = reader.as_ref().map(|reader| reader.store_id().to_owned());
    if let Ok(mut published) = view.branch_reader.lock() {
        *published = reader;
    }
    view.change(|s| {
        s.store_id = store_id;
        s.model = host
            .settings
            .profile
            .model
            .clone()
            .unwrap_or_else(|| "not configured".into());
        s.profile = host.settings.profile_name.clone();
        s.provider = host.settings.profile.kind.label().into();
        s.effort = host
            .settings
            .profile
            .reasoning_effort
            .map_or_else(String::new, |effort| effort.label().into());
        s.root = host.tools.root.display().to_string();
        s.tools = host.tools.mode.to_string();
        s.session_tps = host.session_performance.average_output_tps();
        if let Some(a) = &host.agent {
            s.turns = Arc::new(a.turns().to_vec());
            s.context_blocks = a.context_blocks().to_vec();
            s.needs_inspection = a.needs_reconciliation();
        }
        if let Some(Ok(v)) = &current {
            s.session = Some(v.session.id.clone());
            s.branch_id = Some(v.branch.id.clone());
            s.session_title = Some(v.session.title.clone());
            s.branch = v.branch.name.clone();
            if host.agent.is_none() {
                s.turns = Arc::new(v.history.turns.clone());
                s.context_blocks = v.history.context_blocks.clone();
                s.needs_inspection = v.history.needs_reconciliation;
            }
        }
        if let Some(Err(e)) = &current {
            s.branch_id = None;
            s.status = e.to_string();
            s.turns = Arc::new(vec![]);
            s.context_blocks.clear();
            s.tree.clear();
            s.context_output.clear();
            s.live.clear();
        }
        if host.persistent.is_none() {
            s.branch_id = None;
            s.session = None;
            s.session_title = None;
            s.branch = "memory / main".into();
        }
        if s.event_rail
            .as_ref()
            .is_some_and(|rail| rail.session() != s.session.as_deref().unwrap_or("memory"))
        {
            s.event_rail = None;
        }
        s.delegation_budget = current_budget.clone();
        s.loaded_resources = host.loaded_resources.with_context_blocks(&s.context_blocks);
        if let Some((sessions, subagents)) = session_projection {
            s.sessions = sessions;
            s.subagents = subagents;
        }
    });
}

fn execute(host: &mut Host, view: &Projection, command: Command) -> Result<()> {
    match command {
        Command::Prompt(prompt) => {
            let started = std::time::Instant::now();
            view.change(|s| {
                s.busy = true;
                s.live.clear();
                s.thinking = false;
                s.turn_started = Some(started);
                s.live_tools.clear();
                s.pending_prompt = prompt.clone();
                s.auxiliary_output = None;
                s.status = "Preparing request with current account/tool grants".into();
                s.performance = None;
            });
            if let Err(error) = host.ensure(false, &prompt) {
                view.change(|s| {
                    s.pending_prompt.clear();
                    s.live.clear();
                    s.rejected_prompt = prompt.clone();
                    s.rejected_revision = s.rejected_revision.wrapping_add(1);
                    s.activity(
                        "Prompt was not admitted; rejected input preserved for recovery".into(),
                    );
                });
                return Err(error);
            }
            if host.runtime.is_none() {
                host.runtime = Some(runtime()?);
            }
            let cancel = view
                .cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .ok_or(Error::Storage("prompt cancellation token missing"))?;
            let events = view.clone();
            let event_enabled=host.file_config.ui.show_event_notices;
            let event_registry=crate::tui::extensions::event_registry(&host.file_config.ui);
            // Host::ensure has already selected/created the real Session.
            // A first-turn Snapshot can still have session=None until refresh;
            // use the ORIGINAL bound handle to avoid mislabeling/dropping the
            // first factual rail as a transient "memory" Session.
            let event_session=host.persistent.as_ref()
                .map(|p|p.handle.session_id().to_owned())
                .unwrap_or_else(||"memory".into());
            let turns_before = host.agent.as_ref().map_or(0, |a| a.turns().len());
            let recovery = prompt.clone();
            let first_delta = Arc::new(Mutex::new(None::<std::time::Duration>));
            let first_delta_events = first_delta.clone();
            let result = host
                .runtime
                .as_ref()
                .ok_or(Error::Storage("runtime missing"))?
                .block_on(
                    host.agent
                        .as_mut()
                        .ok_or(Error::Storage("agent missing"))?
                        .prompt(
                            prompt,
                            async move {
                                tokio::select! {_=cancel.cancelled()=>{},_=crate::interrupt()=>{}}
                            },
                            move |event| {
                                events.change(|s| {
                                    if event_enabled {
                                        let session=event_session.clone();
                                        if s.event_rail.as_ref().is_some_and(|r|r.session()!=session) {
                                            s.event_rail=None;
                                        }
                                        if s.event_rail.is_none(){
                                            s.event_rail=aw_ui_extension::status_events::EventRail::new(&session).ok();
                                        }
                                        let ctx=aw_ui_extension::Context{
                                            model:s.model.clone(),profile:s.profile.clone(),
                                            provider:s.provider.clone(),effort:s.effort.clone(),
                                            directory:s.root.clone(),agent:s.session_title.clone().unwrap_or_default(),
                                            branch:s.branch.clone(),tools:s.tools.clone(),session,
                                            state:s.status.clone(),session_tps:s.session_tps,performance:None,
                                        };
                                        if let Some(rail)=&mut s.event_rail {
                                            if let Some(seq)=rail.last_sequence().checked_add(1) {
                                                let facts=aw::extension_status::factual_event_contexts(event,&ctx,seq);
                                                for fact in facts {
                                                    // Presentation errors MUST NOT change the original
                                                    // paid/effect/Turn outcome or replay the event.
                                                    let _=rail.publish(&event_registry,&fact);
                                                }
                                            }
                                        }
                                    }
                                    match event {
                                    AgentEvent::Started { turn_id, model, .. } => {
                                        s.thinking = true;
                                        s.status = format!("Running turn {turn_id}");
                                        s.activity(format!("turn {turn_id} started / {model}"));
                                    }
                                    AgentEvent::Delta { text, .. } => {
                                        s.thinking = false;
                                        let mut first = first_delta_events
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner());
                                        if first.is_none() {
                                            *first = Some(started.elapsed());
                                        }
                                        if s.live.len().saturating_add(text.len())
                                            <= aw::config::MAX_OUTPUT_BYTES
                                        {
                                            s.live.push_str(text);
                                        }
                                    }
                                    AgentEvent::ToolStarted { record, .. } => {
                                        s.thinking = false;
                                        s.status = format!("Tool {} running", record.call.name);
                                        if let Some(existing) = s
                                            .live_tools
                                            .iter_mut()
                                            .find(|item| item.call.id == record.call.id)
                                        {
                                            *existing = record.clone();
                                        } else if s.live_tools.len() < 32 {
                                            s.live_tools.push(record.clone());
                                        }
                                        s.activity(format!(
                                            "tool {} started on {}",
                                            record.call.name, record.execution_site
                                        ));
                                    }
                                    AgentEvent::ToolFinished { record, .. } => {
                                        s.thinking = false;
                                        if let Some(existing) = s
                                            .live_tools
                                            .iter_mut()
                                            .find(|item| item.call.id == record.call.id)
                                        {
                                            *existing = record.clone();
                                        } else if s.live_tools.len() < 32 {
                                            s.live_tools.push(record.clone());
                                        }
                                        s.activity(format!(
                                            "tool {}: {:?}",
                                            record.call.name, record.status
                                        ));
                                    }
                                    AgentEvent::InvocationFinished { record, .. } => {
                                        s.thinking = false;
                                        s.activity(format!(
                                            "model invocation completed / {} / {}",
                                            record.identity.actor.as_str(),
                                            record.identity.account_id
                                        ))
                                    }
                                    AgentEvent::Finished { turn } => {
                                        s.thinking = false;
                                        s.status = format!("Turn {}: {:?}", turn.id, turn.outcome)
                                    }
                                    AgentEvent::Rejected { message } => s.status = message.clone(),
                                    }
                                });
                                Ok(())
                            },
                        ),
                );
            if let Ok(turn) = &result {
                let first = *first_delta.lock().unwrap_or_else(|e| e.into_inner());
                let stats = crate::tui::performance::PerformanceStats::from_turn(
                    turn,
                    started.elapsed(),
                    first,
                );
                host.session_performance.record(&stats);
                let session_tps = host.session_performance.average_output_tps();
                view.change(|s| {
                    s.performance = Some(stats);
                    s.session_tps = session_tps;
                });
            }
            if result.is_err()
                && host
                    .agent
                    .as_ref()
                    .is_some_and(|a| a.turns().len() == turns_before)
            {
                view.change(|s| {
                    s.rejected_prompt = recovery;
                    s.rejected_revision = s.rejected_revision.wrapping_add(1);
                });
            }
            refresh(host, view, true);
            // The completed/rejected attempt is now represented by the real
            // SDK/store history. Do not duplicate it as an additional live item.
            view.change(|s| {
                s.pending_prompt.clear();
                s.live.clear();
                s.thinking = false;
                s.turn_started = None;
                s.live_tools.clear();
            });
            result?;
        }
        Command::Context(command) => {
            view.change(|s| {
                s.busy = true;
                s.context_output.clear();
                s.status = "Explicit context operation; only generate invokes a model".into();
            });
            let cancel = view
                .cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .ok_or(Error::Storage("context cancellation missing"))?;
            let result = host.context_action(command, async move {
                tokio::select! {_=cancel.cancelled()=>{},_=crate::interrupt()=>{}}
            })?;
            let text = serde_json::to_string_pretty(&result)
                .map_err(|_| Error::Storage("context output encoding failed"))?;
            if text.len() > 1024 * 1024 {
                return Err(Error::Limit(
                    "context display exceeds 1 MiB; inspect bounded ranges via CLI",
                ));
            }
            view.change(|s| {
                s.context_output = text;
                s.tree.clear();
                s.status =
                    "Context operation complete; inspect snapshot and apply explicitly".into();
            });
        }
        Command::Refresh => refresh(host, view, true),
        Command::Tree => {
            let p = host.stored()?;
            let branches = p.branches()?;
            let nodes = p.store.tree(&p.actor, p.handle.session_id())?;
            let mut text = format!(
                "Session {}\nBranches (conversation refs, not Git):\n",
                p.handle.session_id()
            );
            for b in branches {
                text.push_str(&format!("  {} -> {} (r{})\n", b.name, b.head, b.revision));
            }
            text.push_str("\nImmutable nodes (parent -> id / kind):\n");
            for n in nodes.iter().take(2000) {
                text.push_str(&format!(
                    "{} -> {} / {} / {}\n",
                    n.parent_id.as_deref().unwrap_or("root"),
                    n.id,
                    n.kind,
                    n.actor.as_str()
                ));
                if text.len() > 256 * 1024 {
                    break;
                }
            }
            if nodes.len() > 2000 {
                text.push_str("[display limited; use --tree/export for bounded complete source]\n");
            }
            view.change(|s| s.tree = text);
        }
        Command::New => {
            host.session_performance.clear();
            host.last_performance = None;
            let next = host
                .persistent
                .as_ref()
                .map(|p| p.new_session("New agent"))
                .transpose()?;
            if let Some(p) = next {
                host.replace(p)?;
            } else if let Some(a) = &mut host.agent {
                a.reset()?;
            }
            view.change(|s| {
                s.pending_prompt.clear();
                s.live.clear();
                s.auxiliary_output = None;
                s.tree.clear();
                s.status = "New agent; current account/tool grants unchanged".into();
            });
            refresh(host, view, true);
        }
        Command::Title(title) => {
            let info = host.stored()?.set_title(&title)?;
            view.change(|s| {
                s.session_title = Some(info.title.clone());
                s.status = format!("Agent title set: {}", info.title);
            });
            refresh(host, view, true);
        }
        Command::SetSessionsArchived { sessions, archived } => {
            if sessions.is_empty() {
                return Ok(());
            }
            if sessions.len() > 128 {
                return Err(Error::Limit("at most 128 Sessions may be updated at once"));
            }
            if let Some(persistent) = &host.persistent {
                persistent
                    .store
                    .set_sessions_archived(&persistent.actor, &sessions, archived)?;
            } else {
                let store = host.sqlite_store()?;
                let actor = aw::sessions::actor(&host.context, false)?;
                store.set_sessions_archived(&actor, &sessions, archived)?;
            }
            let count = sessions.len();
            view.change(|s| {
                s.status = match (archived, count) {
                    (true, 1) => "Session archived for this user; durable history was retained".into(),
                    (true, _) => format!(
                        "{count} Sessions archived for this user; durable history was retained"
                    ),
                    (false, 1) => "Session restored to the Sessions list".into(),
                    (false, _) => format!("{count} Sessions restored to the Sessions list"),
                };
            });
            refresh(host, view, true);
        }
        Command::Resume { session, branch } => {
            let p = if let Some(current) = &host.persistent {
                current.open_saved_session(&session, &branch)?
            } else {
                let store = host.sqlite_store()?;
                aw::sessions::PersistentSession::open_saved(
                    store,
                    aw::sessions::actor(&host.context, false)?,
                    &session,
                    &branch,
                )?
            };
            host.activate_saved_session(p)?;
            view.change(|s| {
                s.auxiliary_output = None;
                s.tree.clear();
                s.status =
                    "Loaded selected Agent using its saved directory, tool root, profile and model"
                        .into();
            });
        }
        Command::Switch(branch) => {
            let next = host.stored()?.switch_branch(&branch)?;
            host.replace(next)?;
            view.change(|s| {
                s.auxiliary_output = None;
                s.tree.clear();
            });
        }
        Command::Fork { name, at } => {
            let next = host.stored()?.fork(&name, at.as_deref())?;
            host.replace(next)?;
            view.change(|s| {
                s.auxiliary_output = None;
                s.tree.clear();
                s.status = "Fork created at current safe boundary; no files copied".into();
            });
        }
        Command::Checkpoint(text) => {
            let next = host.stored()?.checkpoint(&text)?;
            host.replace(next)?;
            view.change(|s| {
                s.tree.clear();
                s.status = "Explicit context checkpoint saved; no model call".into();
            });
        }
        Command::Model(model) => {
            aw::config::validate_model(&model)?;
            if let Some(a) = &mut host.agent {
                a.set_model(&model)?;
            }
            host.settings.profile.model = Some(model);
        }
        Command::Think(effort) => {
            host.set_reasoning_effort(effort)?;
            view.change(|s| {
                s.effort = effort.label().into();
                s.status = format!("Thinking effort: {}", effort.label());
            });
        }
        Command::Side(question) => {
            view.change(|s| {
                s.busy = true;
                s.turn_started = Some(std::time::Instant::now());
                s.auxiliary_output = None;
                s.status = "Running BTW · tool-free and out-of-band".into();
            });
            let prompt = format!(
                "Answer this side question directly using the current conversation context. This answer is out-of-band and will not be added to Agent history. Do not call tools or claim to change workspace state.\n\n{question}"
            );
            let answer =
                host.auxiliary_inference(aw_agent_core::AuxiliaryKind::Side, &prompt)?;
            view.change(|s| {
                s.auxiliary_output = Some(aw::presentation::safe_text(&answer));
                s.status = "BTW answer completed · not added to Agent history".into();
            });
        }
        Command::PersistUiWake => {}
        Command::GenerateTitle => {
            view.change(|s| {
                s.busy = true;
                s.turn_started = Some(std::time::Instant::now());
                s.status = "Generating Agent title · tool-free".into();
            });
            let generated = host.auxiliary_inference(
                aw_agent_core::AuxiliaryKind::Title,
                "Generate a concise title for this Agent session from the conversation context. Return only the title, with no quotes, prefix, markdown, or explanation. Keep it under 100 characters.",
            )?;
            let title = crate::tui::commands::normalize_generated_title(&generated)?;
            let info = host.stored()?.set_title(&title)?;
            view.change(|s| {
                s.session_title = Some(info.title.clone());
                s.status = format!("Generated Agent title: {}", info.title);
            });
        }
        Command::Acknowledge(note) => {
            host.ensure(false, "Inspection acknowledgement")?;
            if host.runtime.is_none() {
                host.runtime = Some(runtime()?);
            }
            host.runtime
                .as_ref()
                .ok_or(Error::Storage("runtime missing"))?
                .block_on(
                    host.agent
                        .as_mut()
                        .ok_or(Error::Storage("agent missing"))?
                        .acknowledge_reconciliation(note),
                )?;
            view.change(|s| {
                s.status = "Explicit inspection acknowledged; no rollback or expanded grants".into()
            });
        }
        Command::GitIndex {
            repo,
            review,
            action,
        } => {
            if view.snapshot().needs_inspection {
                return Err(Error::Denied(
                    "inspect/acknowledge earlier uncertain tool effects before index mutation",
                ));
            }
            repo.apply_index(&review, action).map_err(|e| {
                Error::Config(format!(
                    "Git index action: {e}. Refresh and inspect; no automatic retry."
                ))
            })?;
            view.change(|s| {
                s.status = "Reviewed snapshot applied to index only; refresh Git to inspect".into();
                s.activity(format!(
                    "manual Git {action:?} {} / {}",
                    review.root.display(),
                    review.path_label
                ));
            });
        }
        Command::Stop => {}
    }
    Ok(())
}

#[cfg(test)]
mod publication_benchmarks;

#[cfg(test)]
mod branch_reader_tests {
    use super::*;
    use crate::{config, resources_cli::SessionStoreBackendArg};
    use aw::context::ExecutionContext;
    use std::{path::Path, time::Duration};

    fn host(root: &Path, backend: SessionStoreBackendArg) -> Host {
        let mut profile = config::builtins().remove("llama").unwrap();
        profile.model = Some("fixture".into());
        profile.models = vec!["fixture".into()];
        Host::new(
            config::Settings {
                profile_name: "llama".into(),
                profile,
                system_prompt: String::new(),
                request_timeout: Duration::from_secs(2),
                idle_timeout: Duration::from_secs(2),
                max_output_bytes: 1024,
            },
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            ExecutionContext::capture(root).unwrap(),
            aw::LocalToolOptions::new(root, aw::ToolMode::Off),
            false,
            aw::ToolMode::Off,
            32,
            4,
            64,
            1,
            3600,
            None,
            None,
            None,
            None,
            None,
            None,
            Vec::new(),
            4,
            None,
            None,
            None,
            Some(root.join("state")),
            backend,
            None,
            #[cfg(feature = "jsonl")]
            None,
            #[cfg(feature = "jsonl")]
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn seed(root: &Path) -> (String, String, String) {
        let mut host = host(root, SessionStoreBackendArg::Sqlite);
        let store = host.sqlite_store().unwrap();
        let actor = aw::sessions::actor(&host.context, false).unwrap();
        let binding = aw::sessions::binding(&host.settings, &host.context, &host.tools).unwrap();
        let (session, branch) = store.create(&actor, "Saved fixture", binding).unwrap();
        (store.store_id().unwrap(), session.id, branch.id)
    }

    fn projection(host: &Host) -> Projection {
        let (tick, _) = watch::channel(0);
        Projection {
            data: Arc::new(Mutex::new(Snapshot::new(host))),
            tick,
            cancel: Arc::new(Mutex::new(None)),
            branch_reader: Arc::new(Mutex::new(None)),
        }
    }

    #[test]
    fn unsaved_browse_then_resume_and_new_reuse_one_owner_store() {
        let root = crate::test_support::temp_dir("view-browse-resume");
        let (store_id, session_id, branch_id) = seed(&root);
        {
            let mut host = host(&root, SessionStoreBackendArg::Sqlite);
            let view = projection(&host);
            refresh(&mut host, &view, true);
            assert!(host.persistent.is_none() && host.agent.is_none());
            let snapshot = view.snapshot();
            assert_eq!(snapshot.store_id.as_deref(), Some(store_id.as_str()));
            assert!(snapshot.session.is_none() && snapshot.branch_id.is_none());
            assert_eq!(snapshot.sessions[0].branches[0].id, branch_id);
            let reader = view.branch_reader.lock().unwrap().clone().unwrap();
            let target = super::super::branch_view::Target {
                store_id,
                session_id: session_id.clone(),
                branch_id: branch_id.clone(),
            };
            reader.read(&target).unwrap();
            refresh(&mut host, &view, false);
            assert!(view.branch_reader.lock().unwrap().is_some());
            assert!(host.persistent.is_none() && host.agent.is_none());
            host.resume_session(&session_id, &branch_id).unwrap();
            host.ensure(false, "Existing Agent").unwrap();
            execute(&mut host, &view, Command::New).unwrap();
            assert_ne!(
                host.persistent.as_ref().unwrap().handle.session_id(),
                session_id
            );
            reader.read(&target).unwrap();
            host.resume_session(&session_id, &branch_id).unwrap();
            assert_eq!(
                host.persistent.as_ref().unwrap().handle.session_id(),
                session_id
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsaved_browse_then_first_ensure_creates_only_the_explicit_session() {
        let root = crate::test_support::temp_dir("view-browse-first-prompt");
        let (_, old_session, _) = seed(&root);
        {
            let mut host = host(&root, SessionStoreBackendArg::Sqlite);
            let view = projection(&host);
            refresh(&mut host, &view, true);
            let actor = aw::sessions::actor(&host.context, false).unwrap();
            assert_eq!(host.sqlite_store().unwrap().list(&actor).unwrap().len(), 1);
            host.ensure(false, "Explicit first Agent").unwrap();
            assert_ne!(
                host.persistent.as_ref().unwrap().handle.session_id(),
                old_session
            );
            assert_eq!(host.sqlite_store().unwrap().list(&actor).unwrap().len(), 2);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn browsing_absent_state_or_jsonl_never_initializes_or_falls_back_to_sqlite() {
        let root = crate::test_support::temp_dir("view-no-sqlite-fallback");
        {
            let mut host = host(&root, SessionStoreBackendArg::Sqlite);
            let view = projection(&host);
            refresh(&mut host, &view, true);
            assert!(!root.join("state").exists());
            assert!(view.snapshot().store_id.is_none());
            assert!(view.branch_reader.lock().unwrap().is_none());
        }
        seed(&root);
        {
            let mut host = host(&root, SessionStoreBackendArg::Jsonl);
            let view = projection(&host);
            refresh(&mut host, &view, true);
            assert!(host.sqlite_store.is_none());
            assert!(view.snapshot().sessions.is_empty());
            assert!(view.snapshot().store_id.is_none());
            assert!(view.branch_reader.lock().unwrap().is_none());
            // An unused SQLite authority remains independently openable.
            aw_store::SessionStore::open(&root.join("state")).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_configured_state_clears_publication_without_source_fallback() {
        let root = crate::test_support::temp_dir("view-authority-change");
        seed(&root);
        {
            let mut host = host(&root, SessionStoreBackendArg::Sqlite);
            let view = projection(&host);
            refresh(&mut host, &view, true);
            assert!(view.branch_reader.lock().unwrap().is_some());
            let other = root.join("other-state");
            std::fs::create_dir(&other).unwrap();
            host.state_dir = Some(other.clone());
            refresh(&mut host, &view, true);
            assert!(view.snapshot().store_id.is_none());
            assert!(view.branch_reader.lock().unwrap().is_none());
            assert!(!other.join("sessions.sqlite3").exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
