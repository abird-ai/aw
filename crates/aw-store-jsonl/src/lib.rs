//! Single-owner append-only JSONL session backend.
//!
//! This backend is intentionally smaller than SQLite. It persists conversation
//! admission/effect intent before external actions and preserves unknown outcomes,
//! but does not claim multi-process coordination, ACLs, shared provider accounts,
//! distributed resources or authority-bearing private-state custody.

use aw_agent_core::{
    CompactionPlan, Error, HistoryState, InvocationContext, InvocationIdentity, InvocationPurpose,
    InvocationScope, JournalEntry, JournalFuture, Outcome, PrincipalId, Result, SummaryJournal,
    SummaryResult, Turn, TurnJournal, TurnStart,
};
use aw_store_api::{
    BranchInfo, PortableSession, PortableSessionSource, SessionBackend, SessionBinding,
    SessionCursor, SessionInfo, SessionView, StoreBackend, StoreCapabilities, StoreCapability as C,
    StoreDescriptor, StoreKind,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

const FORMAT: &str = "aw.session-jsonl";
const VERSION: u32 = 1;
const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 2 * 1024 * 1024;
const MAX_SESSIONS: usize = 4096;
const MAIN_BRANCH: &str = "main";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    Header {
        format: String,
        version: u32,
        session: SessionInfo,
        branch: BranchInfo,
        #[serde(default)]
        parent_session: Option<String>,
    },
    Snapshot {
        history: HistoryState,
    },
    TurnStarted {
        start: TurnStart,
    },
    Entry {
        turn_id: u64,
        entry: JournalEntry,
    },
    TurnFinished {
        turn: Turn,
    },
    Acknowledged {
        note: String,
        at: i64,
    },
    TitleChanged {
        title: String,
        at: i64,
    },
    /// Durable exact source + model/account/cost intent before any
    /// external summarizer request; unknown paid outcome is never replayed.
    CompactionStarted {
        plan: CompactionPlan,
        identity: InvocationIdentity,
    },
    /// A separate paid invocation settlement, not a chat Turn.
    CompactionFinished {
        request_id: String,
        result: SummaryResult,
    },
    /// One append-only CAS advance of the active projection. Original
    /// TurnFinished records remain unchanged in the same JSONL log.
    ProjectionCompacted {
        request_id: String,
        expected_next_turn: u64,
    },
}

#[derive(Clone, Debug)]
struct Header {
    session: SessionInfo,
    branch: BranchInfo,
    parent_session: Option<String>,
}

#[derive(Clone, Debug)]
struct Pending {
    start: TurnStart,
    entries: Vec<JournalEntry>,
}

#[derive(Clone, Debug)]
struct Loaded {
    header: Header,
    history: HistoryState,
    pending: Option<Pending>,
    compacting: Option<(CompactionPlan, InvocationIdentity)>,
    settled_compaction: Option<(CompactionPlan, SummaryResult)>,
    used_compaction_ids: std::collections::BTreeSet<String>,
}

struct Inner {
    root: PathBuf,
    _owner_lock: File,
    gate: Mutex<()>,
}

#[derive(Clone)]
pub struct JsonlStore {
    inner: Arc<Inner>,
}

#[derive(Clone)]
pub struct JsonlCursor {
    store: JsonlStore,
    owner: PrincipalId,
    session: String,
}
#[derive(Clone)]
struct JsonlCompactionJournal {
    cursor: JsonlCursor,
    plan: CompactionPlan,
}
fn authorize_compaction(
    loaded: &Loaded,
    owner: &PrincipalId,
    plan: &CompactionPlan,
    identity: &InvocationIdentity,
) -> Result<()> {
    if loaded.header.session.owner != *owner
        || identity.actor != *owner
        || identity.account_id != loaded.header.session.binding.profile_hint
        || identity.inference_executor != "local"
        || plan.session_id != loaded.header.session.id
    {
        return Err(Error::Denied(
            "compaction actor/account/executor differs from current session",
        ));
    }
    if let Some((started, bound_identity)) = &loaded.compacting {
        if started.request_id != plan.request_id || bound_identity != identity {
            return Err(Error::Conflict(
                "another bounded summary attempt is in progress",
            ));
        }
    } else if loaded.pending.is_some()
        || loaded.settled_compaction.is_some()
        || loaded.history.needs_reconciliation
    {
        return Err(Error::Conflict(
            "inspect unresolved Session effects/summary first",
        ));
    }
    plan.check_source(&loaded.history)?;
    Ok(())
}
impl SummaryJournal for JsonlCompactionJournal {
    fn authorize<'a>(
        &'a self,
        input: &'a aw_agent_core::SummaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            if input.digest()? != self.plan.input.digest()?
                || input.sources != self.plan.input.sources
                || input.plan_id != self.plan.input.plan_id
            {
                return Err(Error::Denied(
                    "summary input/digest differs from prepared immutable plan",
                ));
            }
            let _gate = self.cursor.store.gate()?;
            let loaded = self.cursor.store.load(&self.cursor.session)?;
            authorize_compaction(&loaded, &self.cursor.owner, &self.plan, identity)
        })
    }
    fn begin<'a>(
        &'a self,
        input: &'a aw_agent_core::SummaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext> {
        Box::pin(async move {
            if input.digest()? != self.plan.input.digest()? {
                return Err(Error::Denied("summary intent input differs from plan"));
            }
            let _gate = self.cursor.store.gate()?;
            let loaded = self.cursor.store.load(&self.cursor.session)?;
            authorize_compaction(&loaded, &self.cursor.owner, &self.plan, identity)?;
            if loaded.compacting.is_some()
                || loaded.used_compaction_ids.contains(&self.plan.request_id)
            {
                return Err(Error::Conflict(
                    "summary request already attempted; inspect, never replay",
                ));
            }
            self.cursor.store.append(
                &self.cursor.session,
                &Record::CompactionStarted {
                    plan: self.plan.clone(),
                    identity: identity.clone(),
                },
            )?;
            Ok(InvocationContext {
                purpose: InvocationPurpose::Summary {
                    operation_id: self.plan.request_id.clone(),
                    input_sha256: input.digest()?,
                    sources: input.sources.clone(),
                },
                scope: Some(InvocationScope {
                    session_id: self.cursor.session.clone(),
                    branch_id: MAIN_BRANCH.into(),
                }),
                turn_id: self.plan.expected_next_turn,
                ordinal: 1,
                identity: identity.clone(),
            })
        })
    }
    fn finish<'a>(&'a self, result: &'a SummaryResult) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let _gate = self.cursor.store.gate()?;
            let loaded = self.cursor.store.load(&self.cursor.session)?;
            let (plan, identity) = loaded.compacting.as_ref().ok_or(Error::Conflict(
                "summary result not paired with durable paid intent",
            ))?;
            if plan.request_id != self.plan.request_id || identity.actor != self.cursor.owner {
                return Err(Error::Denied(
                    "summary result does not match its actual selected owner",
                ));
            }
            if result.outcome == Outcome::Completed {
                self.plan.projected(&loaded.history, result)?;
            }
            self.cursor.store.append(
                &self.cursor.session,
                &Record::CompactionFinished {
                    request_id: self.plan.request_id.clone(),
                    result: result.clone(),
                },
            )
        })
    }
}

fn now() -> Result<i64> {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Storage("system clock before Unix epoch"))?
        .as_micros();
    i64::try_from(micros).map_err(|_| Error::Limit("timestamp overflow"))
}

fn id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Storage("OS randomness unavailable"))?;
    Ok(format!(
        "{prefix}_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

fn validate_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(Error::Config("invalid JSONL session identifier".into()));
    }
    Ok(())
}

fn validate_title(title: &str) -> Result<()> {
    if title.trim().is_empty()
        || title.len() > 160
        || title.chars().any(|c| c.is_control() && !matches!(c, '\t'))
    {
        return Err(Error::Config(
            "session title needs 1..160 printable bytes".into(),
        ));
    }
    Ok(())
}

fn validate_binding(binding: &SessionBinding) -> Result<()> {
    if !binding.working_directory.is_absolute() || !binding.tool_root.is_absolute() {
        return Err(Error::Config(
            "stored execution paths must be absolute".into(),
        ));
    }
    if binding.system_prompt.len() > aw_agent_core::limits::MAX_PROMPT_BYTES {
        return Err(Error::Limit("system prompt too large"));
    }
    for value in [&binding.profile_hint, &binding.model_hint] {
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(Error::Config("invalid profile/model hint".into()));
        }
    }
    if binding.destination_fingerprint.len() != 64
        || !binding
            .destination_fingerprint
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::Config("invalid destination fingerprint".into()));
    }
    Ok(())
}

fn validate_document(document: &PortableSession) -> Result<()> {
    if document.version != 1 {
        return Err(Error::Config("unsupported portable session version".into()));
    }
    validate_title(&document.title)?;
    if !document.history.context_blocks.is_empty() {
        return Err(Error::Denied(
            "JSONL backend cannot import authority-bound context artifacts; use aw-tree/SQLite",
        ));
    }
    if document.history.turns.len() > aw_agent_core::limits::MAX_ATTEMPTS {
        return Err(Error::Limit("portable session has too many turns"));
    }
    Ok(())
}

impl JsonlStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        if aw_platform::secure_private_files_supported() {
            aw_platform::ensure_private_dir(root)
                .map_err(|_| Error::Denied("JSONL session directory is not private"))?;
        } else if !root.exists() {
            fs::create_dir_all(root)?;
        }
        let root = root.canonicalize()?;
        let lock_path = root.join(".owner.lock");
        let lock = if aw_platform::secure_private_files_supported() {
            match aw_platform::open_private_existing(&lock_path, true) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    aw_platform::open_private_new(&lock_path)?
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)?
        };
        FileExt::try_lock_exclusive(&lock)
            .map_err(|_| Error::Conflict("another aw process owns this JSONL session directory"))?;
        Ok(Self {
            inner: Arc::new(Inner {
                root,
                _owner_lock: lock,
                gate: Mutex::new(()),
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    fn gate(&self) -> Result<MutexGuard<'_, ()>> {
        self.inner
            .gate
            .lock()
            .map_err(|_| Error::Storage("JSONL session lock poisoned"))
    }

    fn file(&self, session: &str) -> Result<PathBuf> {
        validate_id(session)?;
        Ok(self.inner.root.join(format!("{session}.jsonl")))
    }

    fn create_file(&self, session: &str) -> Result<File> {
        let path = self.file(session)?;
        let file = if aw_platform::secure_private_files_supported() {
            aw_platform::open_private_new(&path)?
        } else {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)?
        };
        Ok(file)
    }

    fn append(&self, session: &str, record: &Record) -> Result<()> {
        let bytes =
            serde_json::to_vec(record).map_err(|_| Error::Storage("cannot encode JSONL record"))?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(Error::Limit("JSONL record exceeds 2 MiB"));
        }
        let path = self.file(session)?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(Error::Limit("JSONL session exceeds 32 MiB"));
        }
        let mut file = OpenOptions::new().append(true).open(&path)?;
        file.write_all(&bytes)?;
        file.write_all(
            b"
",
        )?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }

    fn write_new(
        &self,
        mut session: SessionInfo,
        mut branch: BranchInfo,
        parent_session: Option<String>,
        snapshot: Option<HistoryState>,
    ) -> Result<(SessionInfo, BranchInfo)> {
        let mut file = self.create_file(&session.id)?;
        let header = Record::Header {
            format: FORMAT.into(),
            version: VERSION,
            session: session.clone(),
            branch: branch.clone(),
            parent_session,
        };
        write_record(&mut file, &header)?;
        if let Some(history) = snapshot {
            write_record(&mut file, &Record::Snapshot { history })?;
        }
        file.flush()?;
        file.sync_all()?;
        let loaded = self.load(&session.id)?;
        session = loaded.header.session;
        branch = loaded.header.branch;
        Ok((session, branch))
    }

    fn load(&self, session: &str) -> Result<Loaded> {
        let path = self.file(session)?;
        let file = OpenOptions::new().read(true).open(&path)?;
        let length = file.metadata()?.len();
        if length == 0 || length > MAX_FILE_BYTES as u64 {
            return Err(Error::Limit("JSONL session file is empty or oversized"));
        }
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE_BYTES
            || !bytes.ends_with(
                b"
",
            )
        {
            return Err(Error::Limit(
                "JSONL session must be bounded and newline terminated",
            ));
        }
        let text =
            std::str::from_utf8(&bytes).map_err(|_| Error::Config("JSONL must be UTF-8".into()))?;

        let mut lines = text.lines();
        let first = lines
            .next()
            .ok_or(Error::Storage("JSONL session missing header"))?;
        let record: Record = serde_json::from_str(first)
            .map_err(|_| Error::Storage("invalid JSONL session header"))?;
        let (mut session_info, mut branch, parent_session) = match record {
            Record::Header {
                format,
                version,
                session,
                branch,
                parent_session,
            } if format == FORMAT && version == VERSION => (session, branch, parent_session),
            _ => return Err(Error::Storage("unsupported JSONL session header")),
        };
        if session_info.id != session
            || branch.session_id != session
            || branch.id != MAIN_BRANCH
            || branch.name != MAIN_BRANCH
        {
            return Err(Error::Storage("JSONL header identity mismatch"));
        }
        validate_binding(&session_info.binding)?;

        let mut history = HistoryState::default();
        let mut pending: Option<Pending> = None;
        let mut compacting: Option<(CompactionPlan, InvocationIdentity)> = None;
        let mut settled_compaction: Option<(CompactionPlan, SummaryResult)> = None;
        let mut used_compaction_ids = std::collections::BTreeSet::new();
        let mut saw_snapshot = false;
        let mut saw_live = false;
        let mut record_count = 1usize;
        for line in lines {
            record_count = record_count
                .checked_add(1)
                .ok_or(Error::Limit("JSONL record count overflow"))?;
            if record_count > 32768 || line.len() > MAX_RECORD_BYTES {
                return Err(Error::Limit("JSONL record count/size exceeds bound"));
            }
            let record: Record = serde_json::from_str(line)
                .map_err(|_| Error::Storage("invalid JSONL session record"))?;
            match record {
                Record::Header { .. } => {
                    return Err(Error::Storage("duplicate JSONL session header"))
                }
                Record::Snapshot { history: snapshot } => {
                    if saw_snapshot || saw_live || pending.is_some() {
                        return Err(Error::Storage(
                            "JSONL snapshot must occur once after header",
                        ));
                    }
                    aw_agent_core::Conversation::from_history(
                        session_info.binding.system_prompt.clone(),
                        &snapshot,
                    )?;
                    history = snapshot;
                    saw_snapshot = true;
                }
                Record::TurnStarted { start } => {
                    saw_live = true;
                    if compacting.is_some() || settled_compaction.is_some() {
                        return Err(Error::Storage(
                            "pending summary receipt/application blocks another turn",
                        ));
                    }
                    if pending.is_some()
                        || start.turn_id != history.next_turn_id
                        || start.prompt.trim().is_empty()
                        || start.prompt.len() > aw_agent_core::limits::MAX_PROMPT_BYTES
                    {
                        return Err(Error::Storage("invalid JSONL turn admission"));
                    }
                    pending = Some(Pending {
                        start,
                        entries: Vec::new(),
                    });
                }
                Record::Entry { turn_id, entry } => {
                    saw_live = true;
                    let active = pending
                        .as_mut()
                        .ok_or(Error::Storage("journal entry without active turn"))?;
                    if active.start.turn_id != turn_id || active.entries.len() >= 512 {
                        return Err(Error::Storage("journal entry turn mismatch/overflow"));
                    }
                    active.entries.push(entry);
                }
                Record::TurnFinished { turn } => {
                    saw_live = true;
                    let active = pending
                        .take()
                        .ok_or(Error::Storage("finished turn without admission"))?;
                    validate_finished(&active.start, &turn)?;
                    if turn.id != history.next_turn_id {
                        return Err(Error::Storage("finished turn ID mismatch"));
                    }
                    history.turns.push(turn);
                    history.next_turn_id = history
                        .next_turn_id
                        .checked_add(1)
                        .ok_or(Error::Limit("turn ID overflow"))?;
                }
                Record::Acknowledged { note, .. } => {
                    saw_live = true;
                    if note.trim().is_empty() || note.len() > 4096 {
                        return Err(Error::Storage("invalid JSONL acknowledgement"));
                    }
                    if let Some(active) = pending.take() {
                        history.next_turn_id = history
                            .next_turn_id
                            .max(active.start.turn_id.saturating_add(1));
                    }
                    history.needs_reconciliation = false;
                    // An inspected, uncertain paid summary attempt is not
                    // regenerated or applied automatically on restart.
                    compacting = None;
                    settled_compaction = None;
                }
                Record::TitleChanged { title, .. } => {
                    validate_title(&title)?;
                    session_info.title = title;
                }
                Record::CompactionStarted { plan, identity } => {
                    saw_live = true;
                    if pending.is_some()
                        || compacting.is_some()
                        || settled_compaction.is_some()
                        || history.needs_reconciliation
                        || !used_compaction_ids.insert(plan.request_id.clone())
                        || plan.session_id != session
                        || identity.actor != session_info.owner
                        || identity.account_id != session_info.binding.profile_hint
                        || plan.check_source(&history).is_err()
                        || used_compaction_ids.len() > 256
                    {
                        return Err(Error::Storage(
                            "invalid or conflicting durable compaction start",
                        ));
                    }
                    compacting = Some((plan, identity));
                }
                Record::CompactionFinished { request_id, result } => {
                    saw_live = true;
                    let (plan, _) = compacting
                        .take()
                        .ok_or(Error::Storage("summary result has no durable paid intent"))?;
                    if request_id != plan.request_id
                        || (result.outcome == Outcome::Completed
                            && (result.text.trim().is_empty()
                                || result.text.len() > plan.input.max_output_bytes))
                    {
                        return Err(Error::Storage("invalid settled summary result"));
                    }
                    if result.outcome == Outcome::Completed {
                        settled_compaction = Some((plan, result));
                    }
                }
                Record::ProjectionCompacted {
                    request_id,
                    expected_next_turn,
                } => {
                    saw_live = true;
                    let (plan, result) = settled_compaction.take().ok_or(Error::Storage(
                        "projection advance without successful summary",
                    ))?;
                    if request_id != plan.request_id
                        || expected_next_turn != plan.expected_next_turn
                    {
                        return Err(Error::Storage("projection CAS source revision mismatch"));
                    }
                    history = plan.projected(&history, &result)?;
                    history.needs_reconciliation = false;
                    aw_agent_core::Conversation::from_history(
                        session_info.binding.system_prompt.clone(),
                        &history,
                    )?;
                }
            }
        }

        if pending.is_some() || compacting.is_some() || settled_compaction.is_some() {
            history.needs_reconciliation = true;
        }
        // Full projection validity is checked once after the whole durable
        // event log is replayed. Previously it was checked after EVERY
        // TurnFinished while scanning all preceding events: long sessions
        // caused quadratic revalidation inside quadratic repeated file reads.
        aw_agent_core::Conversation::from_history(
            session_info.binding.system_prompt.clone(),
            &history,
        )?;
        session_info.next_turn = history.next_turn_id;
        session_info.updated_at = file_micros(&path)?;
        branch.head = history.turns.last().map_or_else(
            || session_info.root_node.clone(),
            |turn| format!("turn:{}", turn.id),
        );
        // Revision is monotonic even when old immutable Turns leave the
        // active projection following a successful append-only checkpoint.
        branch.revision = history.next_turn_id.saturating_sub(1);
        branch.active_run = pending
            .as_ref()
            .map(|active| format!("jsonl:turn:{}", active.start.turn_id));

        Ok(Loaded {
            header: Header {
                session: session_info,
                branch,
                parent_session,
            },
            history,
            pending,
            compacting,
            settled_compaction,
            used_compaction_ids,
        })
    }

    fn cursor(&self, owner: PrincipalId, session: String) -> Arc<dyn SessionCursor> {
        Arc::new(JsonlCursor {
            store: self.clone(),
            owner,
            session,
        })
    }
}

fn write_record(file: &mut File, record: &Record) -> Result<()> {
    let bytes =
        serde_json::to_vec(record).map_err(|_| Error::Storage("cannot encode JSONL record"))?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(Error::Limit("JSONL record exceeds 2 MiB"));
    }
    file.write_all(&bytes)?;
    file.write_all(
        b"
",
    )?;
    Ok(())
}

fn file_micros(path: &Path) -> Result<i64> {
    let modified = fs::metadata(path)?
        .modified()
        .map_err(|_| Error::Storage("JSONL modification time unavailable"))?;
    let micros = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Storage("JSONL modification time before Unix epoch"))?
        .as_micros();
    i64::try_from(micros).map_err(|_| Error::Limit("timestamp overflow"))
}

fn validate_finished(start: &TurnStart, turn: &Turn) -> Result<()> {
    if turn.id != start.turn_id
        || turn.prompt != start.prompt
        || turn.actor.as_ref() != Some(&start.identity.actor)
        || turn.model != start.identity.model
        || turn.profile != start.identity.account_id
    {
        return Err(Error::Storage("final turn differs from admitted intent"));
    }
    if turn.answer.len() > aw_agent_core::limits::MAX_OUTPUT_BYTES {
        return Err(Error::Limit("turn answer exceeds output bound"));
    }
    Ok(())
}

impl StoreBackend for JsonlStore {
    fn descriptor(&self) -> StoreDescriptor {
        StoreDescriptor {
            kind: StoreKind::Jsonl,
            name: "jsonl".into(),
            durable: true,
            capabilities: StoreCapabilities::new([
                C::Sessions,
                C::Branches,
                C::Forks,
                C::Journal,
                C::ArchiveImport,
                C::ArchiveExport,
                C::EffectRecovery,
            ]),
        }
    }
}

impl SessionBackend for JsonlStore {
    fn create(
        &self,
        owner: &PrincipalId,
        title: &str,
        binding: SessionBinding,
    ) -> Result<(SessionInfo, BranchInfo, Arc<dyn SessionCursor>)> {
        validate_title(title)?;
        validate_binding(&binding)?;
        let _gate = self.gate()?;
        let at = now()?;
        let session_id = id("js")?;
        let session = SessionInfo {
            id: session_id.clone(),
            owner: owner.clone(),
            title: title.into(),
            imported: false,
            workspace_id: "local".into(),
            binding,
            root_node: "root".into(),
            next_turn: 1,
            created_at: at,
            updated_at: at,
        };
        let branch = BranchInfo {
            id: MAIN_BRANCH.into(),
            session_id: session_id.clone(),
            name: MAIN_BRANCH.into(),
            head: "root".into(),
            revision: 0,
            active_run: None,
        };
        let (session, branch) = self.write_new(session, branch, None, None)?;
        let cursor = self.cursor(owner.clone(), session.id.clone());
        Ok((session, branch, cursor))
    }

    fn open(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<Arc<dyn SessionCursor>> {
        if branch != MAIN_BRANCH {
            return Err(Error::Denied(
                "JSONL sessions use one file-backed main branch; fork creates a new session file",
            ));
        }
        let _gate = self.gate()?;
        let loaded = self.load(session)?;
        if &loaded.header.session.owner != owner {
            return Err(Error::Denied(
                "JSONL session belongs to another local principal",
            ));
        }
        Ok(self.cursor(owner.clone(), session.into()))
    }

    fn list(&self, owner: &PrincipalId) -> Result<Vec<SessionInfo>> {
        let _gate = self.gate()?;
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.inner.root)? {
            let entry = entry?;
            if sessions.len() >= MAX_SESSIONS {
                return Err(Error::Limit("too many JSONL sessions"));
            }
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if validate_id(id).is_err() {
                continue;
            }
            let loaded = self.load(id)?;
            if &loaded.header.session.owner == owner {
                sessions.push(loaded.header.session);
            }
        }
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(sessions)
    }

    fn set_title(&self, owner: &PrincipalId, session: &str, title: &str) -> Result<SessionInfo> {
        validate_title(title)?;
        let _gate = self.gate()?;
        let loaded = self.load(session)?;
        if &loaded.header.session.owner != owner {
            return Err(Error::Denied(
                "JSONL session belongs to another local principal",
            ));
        }
        self.append(
            session,
            &Record::TitleChanged {
                title: title.into(),
                at: now()?,
            },
        )?;
        Ok(self.load(session)?.header.session)
    }

    fn branches(&self, owner: &PrincipalId, session: &str) -> Result<Vec<BranchInfo>> {
        let _gate = self.gate()?;
        let loaded = self.load(session)?;
        if &loaded.header.session.owner != owner {
            return Err(Error::Denied(
                "JSONL session belongs to another local principal",
            ));
        }
        Ok(vec![loaded.header.branch])
    }

    fn fork(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
        name: &str,
    ) -> Result<Arc<dyn SessionCursor>> {
        validate_title(name)?;
        if branch != MAIN_BRANCH {
            return Err(Error::Denied(
                "JSONL backend has one branch per session file",
            ));
        }
        let _gate = self.gate()?;
        let loaded = self.load(session)?;
        if &loaded.header.session.owner != owner {
            return Err(Error::Denied(
                "JSONL session belongs to another local principal",
            ));
        }
        if loaded.pending.is_some() || loaded.history.needs_reconciliation {
            return Err(Error::Conflict(
                "inspect/reconcile the current JSONL session before forking",
            ));
        }

        let at = now()?;
        let new_id = id("js")?;
        let mut info = loaded.header.session.clone();
        info.id = new_id.clone();
        info.title = name.into();
        info.imported = false;
        info.root_node = "root".into();
        info.created_at = at;
        info.updated_at = at;
        info.next_turn = loaded.history.next_turn_id;
        let branch = BranchInfo {
            id: MAIN_BRANCH.into(),
            session_id: new_id.clone(),
            name: MAIN_BRANCH.into(),
            head: loaded.header.branch.head,
            revision: loaded.header.branch.revision,
            active_run: None,
        };
        self.write_new(info, branch, Some(session.into()), Some(loaded.history))?;
        Ok(self.cursor(owner.clone(), new_id))
    }

    fn export_document(
        &self,
        owner: &PrincipalId,
        session: &str,
        branch: &str,
    ) -> Result<PortableSession> {
        if branch != MAIN_BRANCH {
            return Err(Error::Denied(
                "JSONL backend has one branch per session file",
            ));
        }
        let _gate = self.gate()?;
        let loaded = self.load(session)?;
        if &loaded.header.session.owner != owner {
            return Err(Error::Denied(
                "JSONL session belongs to another local principal",
            ));
        }
        if loaded.pending.is_some() || loaded.history.needs_reconciliation {
            return Err(Error::Conflict(
                "inspect/reconcile the JSONL session before exporting",
            ));
        }
        Ok(PortableSession {
            version: 1,
            title: loaded.header.session.title,
            cwd: Some(loaded.header.session.binding.working_directory.clone()),
            source: PortableSessionSource {
                backend: StoreKind::Jsonl,
                session_id: session.into(),
                branch_id: MAIN_BRANCH.into(),
                parent_session: loaded.header.parent_session,
            },
            history: loaded.history,
        })
    }

    fn import_document(
        &self,
        owner: &PrincipalId,
        document: PortableSession,
        binding: SessionBinding,
    ) -> Result<(SessionInfo, BranchInfo, Arc<dyn SessionCursor>)> {
        validate_document(&document)?;
        validate_binding(&binding)?;
        aw_agent_core::Conversation::from_history(
            binding.system_prompt.clone(),
            &document.history,
        )?;
        let _gate = self.gate()?;
        let at = now()?;
        let session_id = id("js")?;
        let session = SessionInfo {
            id: session_id.clone(),
            owner: owner.clone(),
            title: document.title,
            imported: true,
            workspace_id: "local".into(),
            binding,
            root_node: "root".into(),
            next_turn: document.history.next_turn_id,
            created_at: at,
            updated_at: at,
        };
        let branch = BranchInfo {
            id: MAIN_BRANCH.into(),
            session_id: session_id.clone(),
            name: MAIN_BRANCH.into(),
            head: document
                .history
                .turns
                .last()
                .map_or_else(|| "root".into(), |turn| format!("turn:{}", turn.id)),
            revision: document.history.turns.len() as u64,
            active_run: None,
        };
        let (session, branch) = self.write_new(
            session,
            branch,
            Some(document.source.session_id),
            Some(document.history),
        )?;
        let cursor = self.cursor(owner.clone(), session.id.clone());
        Ok((session, branch, cursor))
    }
}

impl SessionCursor for JsonlCursor {
    fn session_id(&self) -> &str {
        &self.session
    }

    fn branch(&self) -> Result<BranchInfo> {
        let _gate = self.store.gate()?;
        let loaded = self.store.load(&self.session)?;
        if loaded.header.session.owner != self.owner {
            return Err(Error::Denied("JSONL session owner changed"));
        }
        Ok(loaded.header.branch)
    }

    fn view(&self) -> Result<SessionView> {
        let _gate = self.store.gate()?;
        let loaded = self.store.load(&self.session)?;
        if loaded.header.session.owner != self.owner {
            return Err(Error::Denied("JSONL session owner changed"));
        }
        Ok(SessionView {
            session: loaded.header.session,
            branch: loaded.header.branch,
            history: loaded.history,
        })
    }

    fn journal(&self) -> Arc<dyn TurnJournal> {
        Arc::new(self.clone())
    }
    fn compaction_journal(&self, plan: CompactionPlan) -> Result<Arc<dyn SummaryJournal>> {
        let _gate = self.store.gate()?;
        let loaded = self.store.load(&self.session)?;
        if loaded.pending.is_some()
            || loaded.compacting.is_some()
            || loaded.settled_compaction.is_some()
            || loaded.history.needs_reconciliation
            || loaded.used_compaction_ids.contains(&plan.request_id)
            || loaded.header.session.owner != self.owner
        {
            return Err(Error::Conflict(
                "JSONL compact source is pending, settled or already attempted",
            ));
        }
        plan.check_source(&loaded.history)?;
        if plan.session_id != self.session {
            return Err(Error::Denied("compaction plan for different Session"));
        }
        Ok(Arc::new(JsonlCompactionJournal {
            cursor: self.clone(),
            plan,
        }))
    }
    fn compaction_ready(&self) -> Result<Option<String>> {
        let _gate = self.store.gate()?;
        let loaded = self.store.load(&self.session)?;
        if loaded.header.session.owner != self.owner {
            return Err(Error::Denied("JSONL compaction belongs to another actor"));
        }
        Ok(loaded.settled_compaction.map(|(plan, _)| plan.request_id))
    }
    fn apply_compaction(&self, request_id: &str) -> Result<HistoryState> {
        let _gate = self.store.gate()?;
        let loaded = self.store.load(&self.session)?;
        if loaded.header.session.owner != self.owner
            || loaded.pending.is_some()
            || loaded.compacting.is_some()
        {
            return Err(Error::Denied(
                "cannot compact a different owner or unfinished turn",
            ));
        }
        let (plan, result) = loaded.settled_compaction.ok_or(Error::Conflict(
            "no successfully finished summary to apply without model replay",
        ))?;
        if request_id != plan.request_id {
            return Err(Error::Conflict(
                "summary request ID differs from settled exact plan",
            ));
        }
        let mut next = plan.projected(&loaded.history, &result)?;
        next.needs_reconciliation = false;
        aw_agent_core::Conversation::from_history(
            loaded.header.session.binding.system_prompt.clone(),
            &next,
        )?;
        self.store.append(
            &self.session,
            &Record::ProjectionCompacted {
                request_id: request_id.to_owned(),
                expected_next_turn: plan.expected_next_turn,
            },
        )?;
        Ok(next)
    }
}

impl TurnJournal for JsonlCursor {
    fn validate_context(&self) -> JournalFuture<'_, ()> {
        let this = self.clone();
        Box::pin(async move {
            let _gate = this.store.gate()?;
            let loaded = this.store.load(&this.session)?;
            if loaded.header.session.owner != this.owner {
                return Err(Error::Denied("JSONL session owner changed"));
            }
            Ok(())
        })
    }

    fn validate_context_as<'a>(
        &'a self,
        identity: &'a aw_agent_core::InvocationIdentity,
    ) -> JournalFuture<'a, ()> {
        let this = self.clone();
        let identity = identity.clone();
        Box::pin(async move {
            if identity.actor != this.owner {
                return Err(Error::Denied("JSONL journal caller differs from owner"));
            }
            let _gate = this.store.gate()?;
            let loaded = this.store.load(&this.session)?;
            if loaded.header.session.owner != this.owner {
                return Err(Error::Denied("JSONL session owner changed"));
            }
            Ok(())
        })
    }

    fn invocation_scope(&self) -> Result<Option<InvocationScope>> {
        Ok(Some(InvocationScope {
            session_id: self.session.clone(),
            branch_id: MAIN_BRANCH.into(),
        }))
    }

    fn begin<'a>(&'a self, start: TurnStart) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            let _gate = this.store.gate()?;
            if start.identity.actor != this.owner {
                return Err(Error::Denied("JSONL turn actor differs from session owner"));
            }
            let loaded = this.store.load(&this.session)?;
            if loaded.pending.is_some() || loaded.history.needs_reconciliation {
                return Err(Error::Conflict(
                    "JSONL session needs inspection before another model/tool action",
                ));
            }
            if start.turn_id != loaded.history.next_turn_id {
                return Err(Error::Conflict(
                    "JSONL session advanced; reload before submitting",
                ));
            }
            this.store
                .append(&this.session, &Record::TurnStarted { start })
        })
    }

    fn append<'a>(&'a self, turn_id: u64, entry: JournalEntry) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            let _gate = this.store.gate()?;
            let loaded = this.store.load(&this.session)?;
            let pending = loaded
                .pending
                .ok_or(Error::Conflict("JSONL journal has no active turn"))?;
            if pending.start.turn_id != turn_id {
                return Err(Error::Conflict("JSONL journal turn changed"));
            }
            match &entry {
                JournalEntry::InvocationStarted { ordinal, identity } => {
                    if identity.actor != this.owner || *ordinal == 0 || *ordinal > 32 {
                        return Err(Error::Denied("invalid JSONL invocation identity/ordinal"));
                    }
                }
                JournalEntry::InvocationFinished { record } => {
                    let started = pending.entries.iter().any(|prior| {
                        matches!(
                            prior,
                            JournalEntry::InvocationStarted { ordinal, identity }
                                if *ordinal == record.ordinal && identity == &record.identity
                        )
                    });
                    if !started {
                        return Err(Error::Storage(
                            "JSONL invocation result has no durable start intent",
                        ));
                    }
                }
                JournalEntry::Assistant { message } => {
                    if message.provider_context.is_some() {
                        return Err(Error::Denied(
                            "provider-private continuation cannot enter JSONL",
                        ));
                    }
                }
                JournalEntry::ToolDenied { record } => {
                    aw_agent_core::journal::validate_denial(record)?;
                    if !pending.entries.iter().any(|prior| {
                        matches!(
                            prior,JournalEntry::Assistant {message}
                            if message.tool_calls.iter().any(|call|call==&record.call)
                        )
                    }) || pending.entries.iter().any(|prior| match prior {
                        JournalEntry::ToolStarted { record: old, .. }
                        | JournalEntry::ToolDenied { record: old } => old.call.id == record.call.id,
                        JournalEntry::ToolRevised { original, .. } => original.id == record.call.id,
                        _ => false,
                    }) {
                        return Err(Error::Denied(
                            "no-effect denial requires one unstarted original assistant call",
                        ));
                    }
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
                    if !pending.entries.iter().any(|prior| {
                        matches!(
                            prior,JournalEntry::Assistant{message}
                            if message.tool_calls.iter().any(|call|call==original)
                        )
                    }) || pending.entries.iter().any(|prior| match prior {
                        JournalEntry::ToolStarted { record, .. }
                        | JournalEntry::ToolDenied { record } => record.call.id == original.id,
                        JournalEntry::ToolRevised {
                            original: prior, ..
                        } => prior.id == original.id,
                        _ => false,
                    }) {
                        return Err(Error::Denied(
                            "tool revision must precede any effect and match original assistant",
                        ));
                    }
                }
                JournalEntry::ToolStarted { record, source } => {
                    record.call.validate_envelope()?;
                    if let Some(source) = source {
                        source.validate()?;
                    }
                    if record.status != aw_agent_core::ToolStatus::Running
                        || record.output.is_some()
                        || pending.entries.iter().any(|prior| {
                            matches!(prior,
                            JournalEntry::ToolStarted{record:old, ..}
                            |JournalEntry::ToolDenied{record:old}
                            if old.call.id==record.call.id)
                        })
                    {
                        return Err(Error::Denied(
                            "tool effect intent already started or denied",
                        ));
                    }
                    if let Some((revised, effect, source_id, source_kind, source_epoch)) =
                        pending.entries.iter().find_map(|prior| match prior {
                            JournalEntry::ToolRevised {
                                revised,
                                effect,
                                source_id,
                                source_kind,
                                source_epoch,
                                ..
                            } if revised.id == record.call.id => {
                                Some((revised, effect, source_id, source_kind, source_epoch))
                            }
                            _ => None,
                        })
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
                                "tool intent source/effect changed from approved revision",
                            ));
                        }
                    } else if !pending.entries.iter().any(|prior| {
                        matches!(
                            prior,JournalEntry::Assistant{message}
                            if message.tool_calls.iter().any(|original|original==&record.call)
                        )
                    }) {
                        return Err(Error::Denied(
                            "tool intent lacks exact original Assistant proposal",
                        ));
                    }
                }
                JournalEntry::ToolFinished { record } => {
                    let started = pending.entries.iter().any(|prior| {
                        matches!(prior, JournalEntry::ToolStarted{record:begun, ..}
                            if begun.call==record.call && begun.effect==record.effect
                                && begun.execution_site==record.execution_site)
                    });
                    if !started
                        || matches!(
                            record.status,
                            aw_agent_core::ToolStatus::Running | aw_agent_core::ToolStatus::Denied
                        )
                        || pending.entries.iter().any(|prior| {
                            matches!(prior,
                            JournalEntry::ToolFinished{record:finished}
                            if finished.call.id==record.call.id)
                        })
                    {
                        return Err(Error::Storage(
                            "JSONL tool result lacks exact unique durable effect intent",
                        ));
                    }
                }
            }
            this.store
                .append(&this.session, &Record::Entry { turn_id, entry })
        })
    }

    fn finish<'a>(&'a self, turn: Turn) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            let _gate = this.store.gate()?;
            let loaded = this.store.load(&this.session)?;
            let pending = loaded
                .pending
                .ok_or(Error::Conflict("JSONL journal has no active turn"))?;
            validate_finished(&pending.start, &turn)?;
            this.store
                .append(&this.session, &Record::TurnFinished { turn })
        })
    }

    fn acknowledge<'a>(&'a self, note: String) -> JournalFuture<'a, ()> {
        let this = self.clone();
        Box::pin(async move {
            if note.trim().is_empty() || note.len() > 4096 {
                return Err(Error::Config("inspection note needs 1..4096 bytes".into()));
            }
            let _gate = this.store.gate()?;
            let loaded = this.store.load(&this.session)?;
            if loaded.pending.is_none() && !loaded.history.needs_reconciliation {
                return Err(Error::Conflict(
                    "JSONL session has nothing awaiting inspection",
                ));
            }
            this.store
                .append(&this.session, &Record::Acknowledged { note, at: now()? })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AccountOwner, BillingOwner, CredentialCustody, InvocationIdentity, Outcome, ToolEffect,
        ToolRecord, ToolStatus,
    };

    fn owner() -> PrincipalId {
        PrincipalId::new("local:test").unwrap()
    }

    fn binding(root: &Path) -> SessionBinding {
        SessionBinding {
            working_directory: root.to_owned(),
            tool_root: root.to_owned(),
            system_prompt: String::new(),
            profile_hint: "test".into(),
            model_hint: "model".into(),
            destination_fingerprint: "0".repeat(64),
        }
    }

    fn identity(actor: PrincipalId) -> InvocationIdentity {
        InvocationIdentity {
            actor: actor.clone(),
            account_id: "test".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("local".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "model".into(),
            grant_revision: 1,
        }
    }

    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(id("aw-jsonl-test").unwrap());
        fs::create_dir_all(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        path
    }

    #[test]
    fn create_finish_reload_and_fork_to_new_file() {
        let root = temp();
        let store = JsonlStore::open(&root).unwrap();
        let actor = owner();
        let (session, _, cursor) = store.create(&actor, "first", binding(&root)).unwrap();
        let journal = cursor.journal();
        let start = TurnStart {
            turn_id: 1,
            prompt: "hello".into(),
            identity: identity(actor.clone()),
        };
        futures_lite::future::block_on(journal.begin(start)).unwrap();
        let turn = Turn {
            id: 1,
            prompt: "hello".into(),
            answer: "world".into(),
            model: "model".into(),
            profile: "test".into(),
            outcome: Outcome::Completed,
            meta: None,
            error: None,
            actor: Some(actor.clone()),
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages: Vec::new(),
            needs_reconciliation: false,
        };
        futures_lite::future::block_on(journal.finish(turn)).unwrap();
        assert_eq!(cursor.view().unwrap().history.turns.len(), 1);

        let fork = store
            .fork(&actor, &session.id, MAIN_BRANCH, "fork")
            .unwrap();
        assert_ne!(fork.session_id(), session.id);
        assert_eq!(fork.view().unwrap().history.turns.len(), 1);
        assert!(root.join(format!("{}.jsonl", fork.session_id())).is_file());
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn title_change_is_append_only_owner_metadata() {
        let root = temp();
        let store = JsonlStore::open(&root).unwrap();
        let actor = owner();
        let (session, _, _) = store.create(&actor, "first", binding(&root)).unwrap();
        let updated = store.set_title(&actor, &session.id, "Agent title").unwrap();
        assert_eq!(updated.title, "Agent title");
        assert_eq!(
            store
                .open(&actor, &session.id, MAIN_BRANCH)
                .unwrap()
                .view()
                .unwrap()
                .session
                .title,
            "Agent title"
        );
        assert!(store
            .set_title(
                &PrincipalId::new("local:other").unwrap(),
                &session.id,
                "not allowed",
            )
            .is_err());
        drop(store);

        let reopened = JsonlStore::open(&root).unwrap();
        assert_eq!(
            reopened
                .open(&actor, &session.id, MAIN_BRANCH)
                .unwrap()
                .view()
                .unwrap()
                .session
                .title,
            "Agent title"
        );
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_turn_requires_acknowledgement_not_replay() {
        let root = temp();
        let store = JsonlStore::open(&root).unwrap();
        let actor = owner();
        let (_, _, cursor) = store.create(&actor, "first", binding(&root)).unwrap();
        let journal = cursor.journal();
        let start = TurnStart {
            turn_id: 1,
            prompt: "hello".into(),
            identity: identity(actor.clone()),
        };
        futures_lite::future::block_on(journal.begin(start.clone())).unwrap();
        let tool = ToolRecord {
            call: aw_agent_core::ToolCall {
                id: "call".into(),
                name: "write".into(),
                arguments: serde_json::json!({"path":"x","content":"y"}),
            },
            effect: ToolEffect::Write,
            execution_site: "local".into(),
            status: ToolStatus::Running,
            output: None,
        };
        // Real Agent source order: durable Assistant proposal before any
        // Process/Write intent, including an interrupted-owner fixture.
        let mut assistant = aw_agent_core::Message::text(2, aw_agent_core::Role::Assistant, "");
        assistant.tool_calls = vec![tool.call.clone()];
        futures_lite::future::block_on(
            journal.append(1, JournalEntry::Assistant { message: assistant }),
        )
        .unwrap();
        futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolStarted {
                record: tool,
                source: Some(aw_agent_core::ToolSourceEvidence::trusted_host()),
            },
        ))
        .unwrap();
        assert!(cursor.view().unwrap().history.needs_reconciliation);
        assert!(futures_lite::future::block_on(journal.begin(start)).is_err());
        futures_lite::future::block_on(journal.acknowledge("inspected".into())).unwrap();
        assert!(!cursor.view().unwrap().history.needs_reconciliation);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn denied_call_is_durable_without_started_effect_and_cannot_be_replayed() {
        let root = temp();
        let store = JsonlStore::open(&root).unwrap();
        let actor = owner();
        let (_, _, cursor) = store.create(&actor, "denial", binding(&root)).unwrap();
        let journal = cursor.journal();
        let proposal = aw_agent_core::ToolCall {
            id: "x_denied".into(),
            name: "write".into(),
            arguments: serde_json::json!({"path":"foo"}),
        };
        futures_lite::future::block_on(journal.begin(TurnStart {
            turn_id: 1,
            prompt: "no effect".into(),
            identity: identity(actor.clone()),
        }))
        .unwrap();
        let mut assistant = aw_agent_core::Message::text(7, aw_agent_core::Role::Assistant, "");
        assistant.tool_calls = vec![proposal.clone()];
        futures_lite::future::block_on(
            journal.append(1, JournalEntry::Assistant { message: assistant }),
        )
        .unwrap();
        let denied = ToolRecord {
            call: proposal.clone(),
            effect: ToolEffect::Write,
            execution_site: "local".into(),
            status: ToolStatus::Denied,
            output: Some(aw_agent_core::ToolOutput::error("denied without execution")),
        };
        futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolDenied {
                record: denied.clone(),
            },
        ))
        .unwrap();
        // Any unfinished append-only Turn is conservatively pending. The
        // denial itself has no effect and needs no hazard after true finish.
        assert!(cursor.view().unwrap().history.needs_reconciliation);
        assert!(futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolDenied {
                record: denied.clone()
            }
        ))
        .is_err());
        let mut invalid = denied.clone();
        invalid.status = ToolStatus::Running;
        invalid.output = None;
        assert!(futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolStarted {
                record: invalid,
                source: None,
            },
        ))
        .is_err());
        futures_lite::future::block_on(journal.finish(Turn {
            id: 1,
            prompt: "no effect".into(),
            answer: "error noted".into(),
            model: "model".into(),
            profile: "test".into(),
            outcome: Outcome::Completed,
            meta: None,
            error: None,
            actor: Some(actor.clone()),
            tool_records: vec![denied],
            invocations: Vec::new(),
            context_messages: Vec::new(),
            needs_reconciliation: false,
        }))
        .unwrap();
        assert!(!cursor.view().unwrap().history.needs_reconciliation);
        let session_id = cursor.session_id().to_owned();
        drop(journal);
        drop(cursor);
        drop(store);
        let reopened = JsonlStore::open(&root).unwrap();
        assert!(
            !reopened
                .open(&actor, &session_id, MAIN_BRANCH)
                .unwrap()
                .view()
                .unwrap()
                .history
                .needs_reconciliation
        );
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn tool_revision_needs_original_assistant_and_exact_execution_intent() {
        let root = temp();
        let store = JsonlStore::open(&root).unwrap();
        let actor = owner();
        let (_, _, cursor) = store.create(&actor, "revision", binding(&root)).unwrap();
        let journal = cursor.journal();
        futures_lite::future::block_on(journal.begin(TurnStart {
            turn_id: 1,
            prompt: "safe revision".into(),
            identity: identity(actor.clone()),
        }))
        .unwrap();
        let original = aw_agent_core::ToolCall {
            id: "rev_x".into(),
            name: "read".into(),
            arguments: serde_json::json!({"unsafe":true}),
        };
        let revised = aw_agent_core::ToolCall {
            arguments: serde_json::json!({}),
            ..original.clone()
        };
        let revision = JournalEntry::ToolRevised {
            original: original.clone(),
            revised: revised.clone(),
            source_id: "trusted-host".into(),
            source_kind: "host".into(),
            source_epoch: 1,
            effect: ToolEffect::Read,
        };
        assert!(futures_lite::future::block_on(journal.append(1, revision.clone())).is_err());
        let mut assistant = aw_agent_core::Message::text(8, aw_agent_core::Role::Assistant, "");
        assistant.tool_calls = vec![original.clone()];
        futures_lite::future::block_on(
            journal.append(1, JournalEntry::Assistant { message: assistant }),
        )
        .unwrap();
        futures_lite::future::block_on(journal.append(1, revision.clone())).unwrap();
        assert!(futures_lite::future::block_on(journal.append(1, revision)).is_err());
        let mut intent = ToolRecord {
            call: original,
            effect: ToolEffect::Read,
            execution_site: "local".into(),
            status: ToolStatus::Running,
            output: None,
        };
        assert!(futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolStarted {
                record: intent.clone(),
                source: None,
            }
        ))
        .is_err());
        intent.call = revised;
        // An external source/epoch cannot become the approved original Host
        // execution merely by selecting the right tool name/arguments.
        for source in [
            aw_agent_core::ToolSourceEvidence {
                kind: "unknown".into(),
                id: "trusted-host".into(),
                epoch: 1,
            },
            aw_agent_core::ToolSourceEvidence {
                kind: "extension".into(),
                id: "foreign".into(),
                epoch: 9,
            },
            aw_agent_core::ToolSourceEvidence {
                kind: "host".into(),
                id: "trusted-host".into(),
                epoch: 2,
            },
        ] {
            assert!(futures_lite::future::block_on(journal.append(
                1,
                JournalEntry::ToolStarted {
                    record: intent.clone(),
                    source: Some(source)
                },
            ))
            .is_err());
        }
        futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolStarted {
                record: intent.clone(),
                source: Some(aw_agent_core::ToolSourceEvidence::trusted_host()),
            },
        ))
        .unwrap();
        let mut bad_receipt = intent.clone();
        bad_receipt.status = ToolStatus::Succeeded;
        bad_receipt.call.arguments = serde_json::json!({"other":true});
        bad_receipt.output = Some(aw_agent_core::ToolOutput::text("wrong"));
        assert!(futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolFinished {
                record: bad_receipt
            }
        ))
        .is_err());
        intent.status = ToolStatus::Succeeded;
        intent.output = Some(aw_agent_core::ToolOutput::text("factual"));
        futures_lite::future::block_on(journal.append(
            1,
            JournalEntry::ToolFinished {
                record: intent.clone(),
            },
        ))
        .unwrap();
        assert!(futures_lite::future::block_on(
            journal.append(1, JournalEntry::ToolFinished { record: intent })
        )
        .is_err());
        drop(store);
        let _ = fs::remove_dir_all(root);
    }
}
